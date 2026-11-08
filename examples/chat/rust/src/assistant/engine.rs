//! The interface between Rust and a Swift model engine, and the sink the
//! engine pushes text into.
//!
//! No inference happens in Rust. An engine (Apple Foundation Models now,
//! CoreML-LLM from L2) is a Swift object implementing [`LocalModelEngine`];
//! the app installs it with `AssistantCore::set_engine`. Rust builds the
//! prompt, keeps the history and decides when to stop; the engine turns the
//! prompt into text and pushes it, piece by piece, into a [`TokenSink`].

use std::sync::Arc;

use super::generation::Shared;

/// What an engine is and whether it can run now.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct EngineInfo {
    /// Stable id, recorded with each answer ("apple-foundation-models").
    pub id: String,
    /// The name the app shows ("Apple Intelligence").
    pub name: String,
    /// Whether `generate` can work right now.
    pub available: bool,
    /// Why not, in words the user can act on ("Turn on Apple Intelligence
    /// in Settings"), when `available` is false.
    pub unavailable_reason: Option<String>,
    /// How many tokens the model can read and write in one request. Rust
    /// trims the oldest history to fit.
    pub context_tokens: u32,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PromptRole {
    User,
    Assistant,
}

/// One earlier message, or the new one (always last, from the user).
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PromptTurn {
    pub role: PromptRole,
    pub text: String,
}

/// Everything an engine needs for one answer. Rust has already chosen and
/// trimmed the history; the engine renders it in the model's own format.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct GenerateRequest {
    /// The system instructions: the character and the house rules. Never
    /// trimmed.
    pub instructions: String,
    /// Oldest first; the last turn is the user's new message.
    pub turns: Vec<PromptTurn>,
    /// Sampling temperature, 0 to 1.5.
    pub temperature: f64,
    /// The longest answer, in tokens.
    pub max_tokens: u32,
}

/// Why an engine's generation ended without an answer.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    /// The model cannot run now (Apple Intelligence off, its model still
    /// downloading, an unsupported iPhone).
    #[error("unavailable: {reason}")]
    Unavailable { reason: String },
    /// The model's own safety rules declined the request (Foundation
    /// Models' guardrails). Shown as a refusal, not as a failure.
    #[error("refused: {reason}")]
    Refused { reason: String },
    /// The prompt does not fit the model's context window.
    #[error("too long: {reason}")]
    ContextTooLong { reason: String },
    /// Anything else the engine reports.
    #[error("{reason}")]
    Engine { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for AiError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Engine { reason: err.reason }
    }
}

/// A model engine, implemented in Swift. Rust calls `generate` on a thread
/// of its own, never on the main thread.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait LocalModelEngine: Send + Sync {
    /// Which engine this is and whether it can run now. Cheap; called
    /// before each answer and whenever the Assistant screen appears.
    fn describe(&self) -> EngineInfo;

    /// Writes the answer to `request` into `sink`, piece by piece, and
    /// returns when the answer is complete, or as soon as `sink.push`
    /// returns false (the user pressed Stop): the engine must then stop
    /// computing, not only stop pushing.
    async fn generate(&self, request: GenerateRequest, sink: Arc<TokenSink>)
        -> Result<(), AiError>;
}

/// Where an engine pushes the answer. Rust keeps the text and the timings
/// and hands it on to the screen at most about 20 times a second.
#[derive(uniffi::Object)]
pub struct TokenSink {
    pub(super) shared: Arc<Shared>,
}

#[uniffi::export]
impl TokenSink {
    /// Adds `text` to the answer. Returns false once the generation was
    /// stopped: the engine must end its loop and stop the model. Cheap
    /// (about a microsecond), so push every piece as it comes.
    pub fn push(&self, text: String) -> bool {
        self.shared.push(text)
    }

    /// Whether the generation was stopped. For engines that can check
    /// between steps without pushing (during a long prompt read).
    pub fn is_stopped(&self) -> bool {
        self.shared.is_cancelled()
    }
}
