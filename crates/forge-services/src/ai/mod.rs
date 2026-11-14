//! Forge AI: one way for app code to ask a language model for text,
//! whichever model answers (docs/forge-ai-plan.md).
//!
//! - **One request shape**, the subset of OpenAI's chat completions that
//!   every backend shares: [`ChatRequest`] with system, user and assistant
//!   [`ChatMessage`]s, a length cap and a temperature. The same request goes
//!   to Apple's on-device model, to an OpenAI-compatible server
//!   ([`openai`]), or over Relay to another device.
//! - **One model interface**, [`LanguageModel`]: `describe` says what the
//!   model is and whether it can run now; `generate` writes the answer into
//!   a [`TextOut`] piece by piece and stops as soon as the caller says so.
//!   It blocks, so call it from a worker thread.
//! - **The answer checks** small models need ([`text`]).
//! - **A scripted model for tests** ([`ScriptedModel`]).
//!
//! No inference happens in Rust. Apple's model is a Swift adapter
//! (`native_services!(ai)`, `ForgeServices+Ai.swift`); a server's model runs
//! in that server.

pub mod openai;
mod scripted;
pub mod text;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub use scripted::ScriptedModel;

#[doc(hidden)]
pub use forge_runtime::block_on;

/// Who wrote a message.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The instructions: who the model is and how to answer.
    System,
    User,
    Assistant,
}

/// One message of a conversation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// One request for an answer: OpenAI's chat completions, text only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatRequest {
    /// The model to use, when the backend has several; `None` takes its
    /// default.
    pub model: Option<String>,
    /// Oldest first. System messages first; the last message is the one to
    /// answer (from the user).
    pub messages: Vec<ChatMessage>,
    /// The longest answer, in tokens.
    pub max_tokens: u32,
    /// Sampling temperature, 0 to 1.5.
    pub temperature: f64,
}

impl ChatRequest {
    /// Instructions and one prompt: the shape of most single requests.
    pub fn new(instructions: &str, prompt: impl Into<String>, max_tokens: u32) -> Self {
        let mut messages = Vec::new();
        if !instructions.trim().is_empty() {
            messages.push(ChatMessage::system(instructions));
        }
        messages.push(ChatMessage::user(prompt));
        Self {
            model: None,
            messages,
            max_tokens,
            temperature: 0.7,
        }
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = temperature;
        self
    }

    /// The system messages, joined: what Apple's model takes as its
    /// instructions.
    pub fn instructions(&self) -> String {
        self.messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The message to answer: the last one, when it is the user's.
    pub fn prompt(&self) -> Option<&str> {
        self.messages
            .last()
            .filter(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
    }

    /// Roughly how many tokens the request reads and may write.
    pub fn estimated_tokens(&self) -> usize {
        self.messages
            .iter()
            .map(|m| text::estimate_tokens(&m.content) + 4)
            .sum::<usize>()
            + self.max_tokens as usize
    }
}

/// What a model is, and whether it can answer now.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    /// Stable id, recorded with each answer ("apple-foundation-models",
    /// "qwen3:8b").
    pub id: String,
    /// The name the app shows ("Apple Intelligence").
    pub name: String,
    /// Whether `generate` can work right now.
    pub available: bool,
    /// Why not, in words the user can act on, when `available` is false.
    pub unavailable_reason: Option<String>,
    /// How many tokens one request can read and write together.
    pub context_tokens: u32,
}

/// Why a model gave no answer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    /// The model can't run now (Apple Intelligence off or still
    /// downloading, an unsupported iPhone, a server that is down).
    Unavailable { reason: String },
    /// The model's safety rules declined the request. Shown as a refusal,
    /// not as a failure.
    Refused { reason: String },
    /// The request does not fit the model's context window.
    ContextTooLong { reason: String },
    /// The model is throttled for now (Apple's limits apps in the
    /// background). Try again later; nothing is wrong with the request.
    Busy { reason: String },
    /// Stopped by the caller.
    Stopped,
    /// Anything else the model reports.
    Engine { reason: String },
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { reason } => write!(f, "unavailable: {reason}"),
            Self::Refused { reason } => write!(f, "refused: {reason}"),
            Self::ContextTooLong { reason } => write!(f, "too long: {reason}"),
            Self::Busy { reason } => write!(f, "busy: {reason}"),
            Self::Stopped => write!(f, "stopped"),
            Self::Engine { reason } => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for AiError {}

/// Where a model writes its answer, and how it learns to stop.
pub trait TextOut: Send + Sync {
    /// The next piece of the answer (only the new text). Returns false once
    /// the caller wants no more: the model must stop computing, not only
    /// stop pushing.
    fn push(&self, delta: &str) -> bool;

    /// Whether the caller stopped the answer. For models that can check
    /// between steps without pushing (while reading a long prompt).
    fn is_stopped(&self) -> bool;
}

/// A language model. `generate` blocks until the answer is complete, the
/// caller stops it, or it fails; call it from a worker thread.
pub trait LanguageModel: Send + Sync {
    fn describe(&self) -> ModelInfo;

    fn generate(&self, request: ChatRequest, out: Arc<dyn TextOut>) -> Result<(), AiError>;
}

/// A [`TextOut`] that keeps the whole answer, with a stop switch and an
/// optional listener that hears the answer so far after each piece.
#[derive(Default)]
pub struct Collect {
    text: Mutex<String>,
    pieces: Mutex<u64>,
    stopped: AtomicBool,
    #[allow(clippy::type_complexity)]
    listener: Option<Box<dyn Fn(&str) + Send + Sync>>,
    stop_when: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

impl Collect {
    pub fn new() -> Self {
        Self::default()
    }

    /// Calls `listener` with the whole answer so far after each piece.
    pub fn listen(mut self, listener: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.listener = Some(Box::new(listener));
        self
    }

    /// Stops the answer whenever `stopped` says so (the caller's own
    /// cancellation).
    pub fn stop_when(mut self, stopped: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.stop_when = Some(Box::new(stopped));
        self
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    /// The answer so far.
    pub fn text(&self) -> String {
        self.text.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// How many pieces arrived.
    pub fn pieces(&self) -> u64 {
        *self.pieces.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl TextOut for Collect {
    fn push(&self, delta: &str) -> bool {
        if self.is_stopped() {
            return false;
        }
        let so_far = {
            let mut text = self.text.lock().unwrap_or_else(|p| p.into_inner());
            text.push_str(delta);
            *self.pieces.lock().unwrap_or_else(|p| p.into_inner()) += 1;
            self.listener.as_ref().map(|_| text.clone())
        };
        if let (Some(listener), Some(so_far)) = (&self.listener, so_far) {
            listener(&so_far);
        }
        !self.is_stopped()
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst) || self.stop_when.as_ref().is_some_and(|f| f())
    }
}

/// Asks `model` and returns the whole answer. `stopped` is checked after
/// every piece.
pub fn ask(
    model: &dyn LanguageModel,
    request: ChatRequest,
    stopped: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<String, AiError> {
    let out = Arc::new(Collect::new().stop_when(stopped));
    model.generate(request, Arc::clone(&out) as Arc<dyn TextOut>)?;
    if out.is_stopped() {
        return Err(AiError::Stopped);
    }
    Ok(out.text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_has_instructions_and_a_prompt() {
        let request = ChatRequest::new("Be brief.", "Hello", 50);
        assert_eq!(request.instructions(), "Be brief.");
        assert_eq!(request.prompt(), Some("Hello"));
        assert!(request.estimated_tokens() > 50);
        let bare = ChatRequest::new("  ", "Hi", 10);
        assert_eq!(bare.messages.len(), 1);
        assert_eq!(bare.instructions(), "");
    }

    #[test]
    fn collect_keeps_the_answer_and_stops() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        let out = Collect::new().listen(move |so_far| log.lock().unwrap().push(so_far.to_string()));
        assert!(out.push("Hel"));
        assert!(out.push("lo"));
        assert_eq!(out.text(), "Hello");
        assert_eq!(*heard.lock().unwrap(), vec!["Hel", "Hello"]);
        out.stop();
        assert!(!out.push("!"));
        assert_eq!(out.text(), "Hello");
        assert_eq!(out.pieces(), 2);
    }

    #[test]
    fn ask_returns_the_answer_or_stopped() {
        let model = ScriptedModel::new(["Hello there."]);
        let answer = ask(&model, ChatRequest::new("", "Hi", 10), || false).unwrap();
        assert_eq!(answer, "Hello there.");
        let model = ScriptedModel::new(["Never read."]);
        let err = ask(&model, ChatRequest::new("", "Hi", 10), || true).unwrap_err();
        assert_eq!(err, AiError::Stopped);
    }

    #[test]
    fn errors_read_as_before() {
        assert_eq!(
            AiError::Refused {
                reason: "no".into()
            }
            .to_string(),
            "refused: no"
        );
        assert_eq!(
            AiError::Engine {
                reason: "boom".into()
            }
            .to_string(),
            "boom"
        );
    }

    #[test]
    fn requests_serialise_as_openai_messages() {
        let request = ChatRequest::new("Be brief.", "Hello", 50);
        let json = serde_json::to_value(&request.messages).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {"role": "system", "content": "Be brief."},
                {"role": "user", "content": "Hello"}
            ])
        );
    }
}
