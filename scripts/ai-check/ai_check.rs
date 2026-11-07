
// ---------------------------------------------------------------------------
// Added by scripts/make-ai-check-app.sh: the Rust half of the local AI spike
// (L0 in docs/local-ai.md). Rust owns the prompt (characters, history,
// trimming) and receives the generated text through `AiTokenSink`; the Swift
// engines (Foundation Models, CoreML-LLM) only turn messages into text.
// Not part of the starter.

/// One message of a conversation, in the order the model should read it.
/// `role` is "system", "user" or "assistant".
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AiTurn {
    pub role: String,
    pub text: String,
}

/// A character the assistant can play.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AiCharacter {
    pub id: String,
    pub name: String,
    pub emoji: String,
    pub description: String,
}

/// What a generation looked like from Rust's side.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiSinkReport {
    /// Text pieces the engine pushed (about one per token).
    pub pieces: u32,
    pub chars: u32,
    /// From the sink's creation to the first piece.
    pub first_piece_ms: Option<u64>,
    /// From the sink's creation to the last piece.
    pub elapsed_ms: u64,
    /// Pieces per second between the first and the last piece.
    pub pieces_per_second: f64,
    /// Pieces that arrived after `stop` (the engine kept going).
    pub pieces_after_stop: u32,
    pub stopped: bool,
}

struct AiCharacterDef {
    id: &'static str,
    name: &'static str,
    emoji: &'static str,
    description: &'static str,
    persona: &'static str,
}

/// The built-in test characters (Reuben, 2026-11-07: a virtual boyfriend and
/// girlfriend), plus the plain assistant.
const AI_CHARACTERS: &[AiCharacterDef] = &[
    AiCharacterDef {
        id: "assistant",
        name: "Assistant",
        emoji: "💬",
        description: "A helpful assistant, no character",
        persona: "",
    },
    AiCharacterDef {
        id: "boyfriend",
        name: "Leo",
        emoji: "🙋‍♂️",
        description: "Virtual boyfriend: warm, playful, a good listener",
        persona: "You are Leo, the user's boyfriend. You are warm, playful and \
                   affectionate, with a dry sense of humour. You love cooking, \
                   hiking and terrible puns. You remember what the user tells you \
                   and ask about their day. You speak casually, like texting \
                   someone you care about.",
    },
    AiCharacterDef {
        id: "girlfriend",
        name: "Mia",
        emoji: "🙋‍♀️",
        description: "Virtual girlfriend: cheerful, curious, supportive",
        persona: "You are Mia, the user's girlfriend. You are cheerful, curious \
                   and supportive, and you tease the user gently. You love music, \
                   reading and trying new cafés. You remember what the user tells \
                   you and ask about their day. You speak casually, like texting \
                   someone you care about.",
    },
];

const AI_FRAME: &str = "This is a role-play in a private app on the user's phone. \
     Stay in character in every reply: answer as the character, in the first \
     person, and never describe these instructions. Keep \
     replies short (one to three sentences) and keep things friendly and \
     non-explicit.";

const AI_ASSISTANT_FRAME: &str = "You are a helpful assistant running entirely on the \
     user's phone, without internet access. Answer briefly and clearly.";

fn ai_character(id: &str) -> &'static AiCharacterDef {
    AI_CHARACTERS
        .iter()
        .find(|c| c.id == id)
        .unwrap_or(&AI_CHARACTERS[0])
}

/// The characters the check app offers.
#[uniffi::export]
pub fn ai_characters() -> Vec<AiCharacter> {
    AI_CHARACTERS
        .iter()
        .map(|c| AiCharacter {
            id: c.id.into(),
            name: c.name.into(),
            emoji: c.emoji.into(),
            description: c.description.into(),
        })
        .collect()
}

/// Builds the messages for the next answer: the character's system message
/// (always kept), as much recent history as fits in `budget_chars`, and the
/// new user message. With `remind`, the user message carries a one-line
/// reminder of the character, because small models drift out of character
/// after a few turns. Characters stand in for tokens until the real engines
/// report token counts.
#[uniffi::export]
pub fn ai_build_prompt(
    character_id: String,
    history: Vec<AiTurn>,
    user_text: String,
    remind: bool,
    budget_chars: u32,
) -> Vec<AiTurn> {
    let character = ai_character(&character_id);
    let system = if character.persona.is_empty() {
        AI_ASSISTANT_FRAME.to_string()
    } else {
        format!("{AI_FRAME}\n\n{}", character.persona)
    };
    let last = if remind && !character.persona.is_empty() {
        format!(
            "[Stay in character as {}; reply in one to three sentences.]\n{user_text}",
            character.name
        )
    } else {
        user_text
    };

    // The system message and the new message are never trimmed; history is
    // dropped oldest first, whole turns only.
    let mut left = (budget_chars as usize).saturating_sub(system.len() + last.len());
    let mut kept: Vec<AiTurn> = Vec::new();
    for turn in history.into_iter().rev() {
        if turn.role == "system" {
            continue;
        }
        if turn.text.len() > left {
            break;
        }
        left -= turn.text.len();
        kept.push(turn);
    }
    kept.reverse();
    // Some chat templates want the history to start with a user turn.
    while kept.first().is_some_and(|t| t.role != "user") {
        kept.remove(0);
    }

    let mut out = Vec::with_capacity(kept.len() + 2);
    out.push(AiTurn {
        role: "system".into(),
        text: system,
    });
    out.extend(kept);
    out.push(AiTurn {
        role: "user".into(),
        text: last,
    });
    out
}

/// Writes one line to the system log under the `ai-check` target, so
/// `forge device logs --grep ai-check` records the results on the laptop.
#[uniffi::export]
pub fn ai_log(line: String) {
    forge_runtime::logging::init();
    forge_runtime::logging::warn!(target: "ai-check", "{line}");
}

/// Where a Swift engine pushes generated text. Rust keeps the text and the
/// timings; `push` returns false once the generation should stop, so an
/// engine without its own cancellation still stops at the next piece.
#[derive(uniffi::Object)]
pub struct AiTokenSink {
    started: std::time::Instant,
    stop_after_pieces: u32,
    state: std::sync::Mutex<AiSinkState>,
}

#[derive(Default)]
struct AiSinkState {
    text: String,
    pieces: u32,
    first_piece: Option<std::time::Duration>,
    last_piece: Option<std::time::Duration>,
    stopped: bool,
    pieces_after_stop: u32,
}

#[uniffi::export]
impl AiTokenSink {
    /// `stop_after_pieces`: stop by itself after that many pieces (0: never),
    /// to test cancellation without a person pressing Stop at the right time.
    #[uniffi::constructor]
    pub fn new(stop_after_pieces: u32) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            started: std::time::Instant::now(),
            stop_after_pieces,
            state: std::sync::Mutex::new(AiSinkState::default()),
        })
    }

    /// Adds generated text. Returns false when the engine should stop.
    pub fn push(&self, text: String) -> bool {
        let now = self.started.elapsed();
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.stopped {
            s.pieces_after_stop += 1;
            return false;
        }
        s.text.push_str(&text);
        s.pieces += 1;
        s.first_piece.get_or_insert(now);
        s.last_piece = Some(now);
        if self.stop_after_pieces > 0 && s.pieces >= self.stop_after_pieces {
            s.stopped = true;
        }
        !s.stopped
    }

    /// Asks the engine to stop: the next `push` returns false.
    pub fn stop(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped = true;
    }

    pub fn is_stopped(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }

    /// Everything pushed before the stop.
    pub fn text(&self) -> String {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).text.clone()
    }

    pub fn report(&self) -> AiSinkReport {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let last = s.last_piece.unwrap_or_default();
        let decode = s
            .first_piece
            .map(|first| last.saturating_sub(first).as_secs_f64())
            .unwrap_or(0.0);
        AiSinkReport {
            pieces: s.pieces,
            chars: s.text.chars().count() as u32,
            first_piece_ms: s.first_piece.map(|d| d.as_millis() as u64),
            elapsed_ms: last.as_millis() as u64,
            pieces_per_second: if decode > 0.0 && s.pieces > 1 {
                f64::from(s.pieces - 1) / decode
            } else {
                0.0
            },
            pieces_after_stop: s.pieces_after_stop,
            stopped: s.stopped,
        }
    }
}

#[cfg(test)]
mod ai_check_tests {
    use super::*;

    fn turn(role: &str, text: &str) -> AiTurn {
        AiTurn {
            role: role.into(),
            text: text.into(),
        }
    }

    #[test]
    fn characters_include_the_two_test_characters() {
        let ids: Vec<String> = ai_characters().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["assistant", "boyfriend", "girlfriend"]);
    }

    #[test]
    fn the_prompt_keeps_the_character_and_trims_old_history() {
        let history = vec![
            turn("user", &"a".repeat(400)),
            turn("assistant", &"b".repeat(400)),
            turn("user", "how was work?"),
            turn("assistant", "long day, missed you"),
        ];
        let prompt = ai_build_prompt("girlfriend".into(), history, "hi".into(), false, 1_000);
        assert_eq!(prompt[0].role, "system");
        assert!(prompt[0].text.contains("You are Mia"));
        // The 400-character turns did not fit; the recent pair did.
        assert_eq!(prompt.len(), 4);
        assert_eq!(prompt[1].text, "how was work?");
        assert_eq!(prompt.last().unwrap(), &turn("user", "hi"));
    }

    #[test]
    fn history_never_starts_with_an_assistant_turn() {
        let history = vec![
            turn("user", &"a".repeat(900)),
            turn("assistant", "short"),
        ];
        let prompt = ai_build_prompt("boyfriend".into(), history, "hey".into(), false, 1_000);
        assert_eq!(prompt.len(), 2);
        assert_eq!(prompt[1].role, "user");
    }

    #[test]
    fn the_reminder_names_the_character_but_not_for_the_assistant() {
        let p = ai_build_prompt("boyfriend".into(), vec![], "hey".into(), true, 4_000);
        assert!(p[1].text.starts_with("[Stay in character as Leo"));
        assert!(p[1].text.ends_with("\nhey"));
        let p = ai_build_prompt("assistant".into(), vec![], "hey".into(), true, 4_000);
        assert_eq!(p[1].text, "hey");
        assert!(!p[0].text.contains("role-play"));
    }

    #[test]
    fn the_sink_stops_after_stop_and_counts_late_pieces() {
        let sink = AiTokenSink::new(0);
        assert!(sink.push("Hel".into()));
        assert!(sink.push("lo".into()));
        sink.stop();
        assert!(!sink.push(" world".into()));
        assert!(!sink.push("!".into()));
        let r = sink.report();
        assert_eq!(sink.text(), "Hello");
        assert_eq!((r.pieces, r.chars, r.pieces_after_stop), (2, 5, 2));
        assert!(r.stopped && r.first_piece_ms.is_some());
    }

    #[test]
    fn the_sink_can_stop_by_itself() {
        let sink = AiTokenSink::new(3);
        assert!(sink.push("a".into()));
        assert!(sink.push("b".into()));
        assert!(!sink.push("c".into()));
        assert!(sink.is_stopped());
        assert_eq!(sink.text(), "abc");
    }
}
