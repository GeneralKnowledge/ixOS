//! What the assistant asks the model, and how Chat's request becomes Forge
//! AI's.
//!
//! No inference happens in Rust. The model is a `native::ai::AiModel`
//! (Apple Foundation Models through `ForgeAppleModel` in Swift, from
//! `native_services!(ai)`); the app installs it with
//! `AssistantCore::set_engine`. Rust builds the prompt, keeps the history
//! and decides when to stop; the model turns the request into text and
//! pushes it, piece by piece, into a `TextSink` that writes into the
//! answer being generated.

pub use crate::native::ai::{
    AiMessage, AiModel, AiModelError, AiModelInfo, AiRequest, AiRole, TextOut, TextSink,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PromptRole {
    User,
    Assistant,
}

/// One earlier message, or the new one (always last, from the user).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptTurn {
    pub role: PromptRole,
    pub text: String,
}

/// Everything the model needs for one answer. Rust has already chosen and
/// trimmed the history; the model renders it in its own format.
#[derive(Clone, Debug, PartialEq)]
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

impl From<GenerateRequest> for AiRequest {
    fn from(request: GenerateRequest) -> Self {
        let mut messages = vec![AiMessage {
            role: AiRole::System,
            content: request.instructions,
        }];
        messages.extend(request.turns.into_iter().map(|turn| AiMessage {
            role: match turn.role {
                PromptRole::User => AiRole::User,
                PromptRole::Assistant => AiRole::Assistant,
            },
            content: turn.text,
        }));
        Self {
            model: None,
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
        }
    }
}

impl From<AiRequest> for GenerateRequest {
    fn from(request: AiRequest) -> Self {
        let mut instructions = Vec::new();
        let mut turns = Vec::new();
        for message in request.messages {
            match message.role {
                AiRole::System => instructions.push(message.content),
                AiRole::User => turns.push(PromptTurn {
                    role: PromptRole::User,
                    text: message.content,
                }),
                AiRole::Assistant => turns.push(PromptTurn {
                    role: PromptRole::Assistant,
                    text: message.content,
                }),
            }
        }
        Self {
            instructions: instructions.join("\n\n"),
            turns,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        }
    }
}
