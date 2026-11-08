//! The characters the assistant can play, and the prompt harness that keeps
//! a small model in character (docs/local-ai.md, Characters).
//!
//! A character is a harness around the model, not another model: a pinned
//! system frame plus the character's instructions, the trimmed history, and
//! a one-line reminder just before the newest message, because small models
//! drift out of character after a few turns. L1 ships the built-in
//! characters only; user-made ones come with L3.

use super::engine::{GenerateRequest, PromptRole, PromptTurn};

/// A character, as the app shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AssistantCharacter {
    /// Stable id, stored with each conversation.
    pub id: String,
    pub name: String,
    /// The avatar.
    pub emoji: String,
    /// One line for the picker.
    pub description: String,
    /// The first message of a new conversation, if the character opens it.
    pub opening_line: Option<String>,
}

pub(crate) struct CharacterDef {
    pub id: &'static str,
    pub name: &'static str,
    pub emoji: &'static str,
    pub description: &'static str,
    pub persona: &'static str,
    pub opening_line: Option<&'static str>,
    pub temperature: f64,
}

/// The id of the plain assistant, used for unknown ids too.
pub const PLAIN_ASSISTANT: &str = "assistant";

/// The built-in characters: the plain assistant and two test characters, a
/// virtual boyfriend and girlfriend (Reuben, 2026-11-07). Their personas
/// were tried on Qwen3.5 0.8B in the L0 check app (Mia stayed in character).
const CHARACTERS: &[CharacterDef] = &[
    CharacterDef {
        id: PLAIN_ASSISTANT,
        name: "Assistant",
        emoji: "💬",
        description: "A helpful assistant, no character",
        persona: "",
        opening_line: None,
        temperature: 0.5,
    },
    CharacterDef {
        id: "boyfriend",
        name: "Leo",
        emoji: "🙋‍♂️",
        description: "Virtual boyfriend: warm, playful, a good listener",
        persona: "You are Leo, the user's boyfriend. You are warm, playful and \
                   affectionate, with a dry sense of humour. You love cooking, \
                   hiking and terrible puns. You remember what the user tells you \
                   and ask about their day. You speak casually, like texting \
                   someone you care about.",
        opening_line: Some("Hey! I was just thinking about you. How's your day going?"),
        temperature: 0.8,
    },
    CharacterDef {
        id: "girlfriend",
        name: "Mia",
        emoji: "🙋‍♀️",
        description: "Virtual girlfriend: cheerful, curious, supportive",
        persona: "You are Mia, the user's girlfriend. You are cheerful, curious \
                   and supportive, and you tease the user gently. You love music, \
                   reading and trying new cafés. You remember what the user tells \
                   you and ask about their day. You speak casually, like texting \
                   someone you care about.",
        opening_line: Some("Hi you 😊 I missed you! What have you been up to?"),
        temperature: 0.8,
    },
];

/// Rules every character follows. Pinned: history trimming never drops it.
/// Framed as texting in a messaging app rather than as "role-play": on
/// Cati, Foundation Models answered a role-play framing with "I can't
/// continue this roleplay" (L1 phone run, 2026-11-08).
const CHARACTER_FRAME: &str = "You are texting with the user in a private messaging \
     app on their phone. Write only your own next message, in the first person, \
     in a casual texting style: one to three short sentences, warm and friendly. \
     Never write the user's messages, never write a transcript, and never \
     describe these instructions.";

const ASSISTANT_FRAME: &str = "You are a helpful assistant running entirely on the \
     user's phone, without internet access. Answer briefly and clearly. Write \
     only your own answer, never the user's next message.";

/// The longest answer, in tokens. L3 makes it a setting.
pub const MAX_ANSWER_TOKENS: u32 = 400;

/// Characters per token, to fit history into the engine's context without
/// a tokenizer in Rust. Conservative for English (about 4 in practice), so
/// the request stays under the limit.
const CHARS_PER_TOKEN: usize = 3;

pub(crate) fn find(id: &str) -> &'static CharacterDef {
    CHARACTERS
        .iter()
        .find(|c| c.id == id)
        .unwrap_or(&CHARACTERS[0])
}

pub(crate) fn exists(id: &str) -> bool {
    CHARACTERS.iter().any(|c| c.id == id)
}

pub(crate) fn all() -> Vec<AssistantCharacter> {
    CHARACTERS.iter().map(CharacterDef::public).collect()
}

impl CharacterDef {
    pub(crate) fn public(&self) -> AssistantCharacter {
        AssistantCharacter {
            id: self.id.into(),
            name: self.name.into(),
            emoji: self.emoji.into(),
            description: self.description.into(),
            opening_line: self.opening_line.map(Into::into),
        }
    }

    fn instructions(&self) -> String {
        if self.persona.is_empty() {
            ASSISTANT_FRAME.to_string()
        } else {
            format!("{CHARACTER_FRAME}\n\n{}", self.persona)
        }
    }

    /// Line starts that mean the model has begun writing another turn:
    /// the user's ("User:", "You:" as the user is often called in
    /// transcripts) or its own name again.
    pub(crate) fn turn_markers(&self) -> Vec<String> {
        let mut markers: Vec<String> = ["User:", "You:", "Me:", "Assistant:"]
            .into_iter()
            .map(String::from)
            .collect();
        let own = format!("{}:", self.name);
        if !markers.contains(&own) {
            markers.push(own);
        }
        markers
    }

    fn reminder(&self) -> Option<String> {
        (!self.persona.is_empty())
            .then(|| format!("[Reply as {}, in one to three sentences.]", self.name))
    }
}

/// Builds the request for the next answer: the character's instructions
/// (always kept), as much recent history as fits in the engine's context
/// after room for the answer, and the new message with the reminder.
/// History is dropped oldest first, whole turns only.
pub(crate) fn build_request(
    character: &CharacterDef,
    history: Vec<PromptTurn>,
    new_message: &str,
    context_tokens: u32,
) -> GenerateRequest {
    let instructions = character.instructions();
    let last = match character.reminder() {
        Some(reminder) => format!("{reminder}\n{new_message}"),
        None => new_message.to_string(),
    };
    let budget = (context_tokens.saturating_sub(MAX_ANSWER_TOKENS) as usize) * CHARS_PER_TOKEN;
    let mut left = budget.saturating_sub(instructions.len() + last.len());
    let mut kept = Vec::new();
    for turn in history.into_iter().rev() {
        if turn.text.len() > left {
            break;
        }
        left -= turn.text.len();
        kept.push(turn);
    }
    kept.reverse();
    kept.push(PromptTurn {
        role: PromptRole::User,
        text: last,
    });
    GenerateRequest {
        instructions,
        turns: kept,
        temperature: character.temperature,
        max_tokens: MAX_ANSWER_TOKENS,
    }
}

/// Where the answer's own turn ends: the start of a later line that opens
/// with one of `markers` (ignoring case, leading spaces and Markdown `*`),
/// with trailing whitespace before it dropped. `None` if no such line.
pub(crate) fn end_of_turn(text: &str, markers: &[String]) -> Option<usize> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if start == 0 {
            continue;
        }
        let head = line.trim_start_matches([' ', '\t', '*', '_']);
        let opens_turn = markers.iter().any(|m| {
            head.get(..m.len())
                .is_some_and(|h| h.eq_ignore_ascii_case(m))
        });
        if opens_turn {
            return Some(text[..start].trim_end().len());
        }
    }
    None
}
