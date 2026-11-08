use std::sync::atomic::AtomicU32;
use std::sync::mpsc;
use std::time::Duration;

use forge_runtime::block_on;

use super::*;

/// A scripted engine: pushes `pieces` with `delay` between them, then
/// returns `result`. Records every request.
struct FakeEngine {
    available: bool,
    context_tokens: u32,
    pieces: Vec<String>,
    delay: Duration,
    result: Result<(), AiError>,
    /// Keeps pushing after `push` returned false (a broken engine).
    ignore_stop: bool,
    requests: Mutex<Vec<GenerateRequest>>,
    /// Pieces it computed after being told to stop.
    computed_after_stop: AtomicU32,
}

impl FakeEngine {
    fn new(pieces: &[&str]) -> Self {
        Self {
            available: true,
            context_tokens: 4096,
            pieces: pieces.iter().map(|p| p.to_string()).collect(),
            delay: Duration::ZERO,
            result: Ok(()),
            ignore_stop: false,
            requests: Mutex::new(Vec::new()),
            computed_after_stop: AtomicU32::new(0),
        }
    }

    fn last_request(&self) -> GenerateRequest {
        self.requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a request")
    }
}

#[async_trait::async_trait]
impl LocalModelEngine for FakeEngine {
    fn describe(&self) -> EngineInfo {
        EngineInfo {
            id: "fake".into(),
            name: "Fake".into(),
            available: self.available,
            unavailable_reason: (!self.available).then(|| "Turn on the fake model".into()),
            context_tokens: self.context_tokens,
        }
    }

    async fn generate(
        &self,
        request: GenerateRequest,
        sink: Arc<TokenSink>,
    ) -> Result<(), AiError> {
        self.requests.lock().unwrap().push(request);
        let mut stopped = false;
        for piece in &self.pieces {
            std::thread::sleep(self.delay);
            if stopped {
                self.computed_after_stop.fetch_add(1, Ordering::SeqCst);
            }
            if !sink.push(piece.clone()) {
                stopped = true;
                if !self.ignore_stop {
                    return Err(AiError::Engine {
                        reason: "cancelled".into(),
                    });
                }
            }
        }
        self.result.clone()
    }
}

fn core_with(engine: FakeEngine) -> (Arc<AssistantCore>, Arc<FakeEngine>) {
    let core = AssistantCore::in_memory();
    let engine = Arc::new(engine);
    core.set_engine(engine.clone());
    (core, engine)
}

/// Reads a generation to the end: the text from `Text` updates, and the
/// final message.
fn drain(generation: &AssistantGeneration) -> (String, usize, AssistantMessage) {
    let mut text = String::new();
    let mut deltas = 0;
    loop {
        match block_on(generation.next()) {
            Some(AssistantUpdate::Text { text: piece }) => {
                text.push_str(&piece);
                deltas += 1;
            }
            Some(AssistantUpdate::Finished { message }) => {
                assert_eq!(
                    block_on(generation.next()),
                    None,
                    "Finished is the last update"
                );
                return (text, deltas, message);
            }
            None => panic!("ended without Finished"),
        }
    }
}

#[test]
fn offers_the_plain_assistant_and_the_two_test_characters() {
    let core = AssistantCore::in_memory();
    let names: Vec<_> = core.characters().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Assistant", "Leo", "Mia"]);
}

#[test]
fn a_character_opens_the_conversation() {
    let core = AssistantCore::in_memory();
    let mia = core.start_conversation("girlfriend".into()).unwrap();
    assert_eq!(mia.character.name, "Mia");
    let opening = mia.last_message.expect("Mia says hello");
    assert_eq!(opening.role, AssistantRole::Assistant);

    let plain = core.start_conversation(PLAIN.into()).unwrap();
    assert!(plain.last_message.is_none());
    assert!(core.messages(plain.id.clone()).unwrap().is_empty());

    let list = core.conversations().unwrap();
    assert_eq!(list.len(), 2);
    assert!(matches!(
        core.start_conversation("nobody".into()),
        Err(AssistantError::NotFound { .. })
    ));
}

const PLAIN: &str = characters::PLAIN_ASSISTANT;

#[test]
fn an_answer_streams_and_is_saved() {
    let (core, _) = core_with(FakeEngine::new(&["Hello", ", ", "world", "!"]));
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core
        .send(conversation.id.clone(), "  Hi there  ".into())
        .unwrap();

    // Saved at once: the message and an empty answer being written.
    let (text, _, message) = drain(&generation);
    assert_eq!(text, "Hello, world!");
    assert_eq!(message.text, "Hello, world!");
    assert_eq!(message.state, AssistantMessageState::Done);
    let stats = message.stats.clone().expect("stats");
    assert_eq!(stats.engine_id, "fake");
    assert_eq!(stats.pieces, 4);
    assert!(stats.first_text_ms.is_some());

    let messages = core.messages(conversation.id.clone()).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, AssistantRole::User);
    assert_eq!(messages[0].text, "Hi there");
    assert_eq!(messages[1], message);
    assert!(core.current_generation().is_none());
}

#[test]
fn text_reaches_the_screen_in_batches_not_per_piece() {
    let pieces: Vec<String> = (0..200).map(|i| format!("{i} ")).collect();
    let refs: Vec<&str> = pieces.iter().map(String::as_str).collect();
    let mut engine = FakeEngine::new(&refs);
    engine.delay = Duration::from_millis(1);
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id, "Count".into()).unwrap();
    let (text, deltas, message) = drain(&generation);
    assert_eq!(text, pieces.concat());
    assert_eq!(message.text, text);
    // 200 pieces over at least 200 ms, at most one delivery per 50 ms.
    assert!(deltas < 50, "{deltas} deliveries for 200 pieces");
}

#[test]
fn the_character_is_pinned_and_reminded_and_history_is_sent() {
    let (core, engine) = core_with(FakeEngine::new(&["Aww, tell me more!"]));
    let mia = core.start_conversation("girlfriend".into()).unwrap();
    drain(
        &core
            .send(mia.id.clone(), "I had a long day".into())
            .unwrap(),
    );
    drain(&core.send(mia.id.clone(), "Work was busy".into()).unwrap());

    let request = engine.last_request();
    assert!(request.instructions.contains("You are Mia"));
    assert!(request
        .instructions
        .contains("Never write the user's messages"));
    assert!(!request.instructions.contains("role-play"));
    let roles: Vec<_> = request.turns.iter().map(|t| t.role).collect();
    use PromptRole::{Assistant as A, User as U};
    // Opening line, first exchange, the new message.
    assert_eq!(roles, [A, U, A, U]);
    assert_eq!(request.turns[1].text, "I had a long day");
    let last = &request.turns[3].text;
    assert!(last.starts_with("[Reply as Mia"), "{last}");
    assert!(last.ends_with("Work was busy"));
    assert_eq!(request.temperature, 0.8);
}

#[test]
fn the_plain_assistant_gets_no_reminder() {
    let (core, engine) = core_with(FakeEngine::new(&["4"]));
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    drain(&core.send(conversation.id, "2+2?".into()).unwrap());
    let request = engine.last_request();
    assert_eq!(request.turns.len(), 1);
    assert_eq!(request.turns[0].text, "2+2?");
}

#[test]
fn old_history_is_trimmed_to_the_context_but_the_character_is_kept() {
    let long_answer = "x".repeat(600);
    let mut engine = FakeEngine::new(&[long_answer.as_str()]);
    // (700 - 400 answer tokens) * 3 chars = 900 chars for everything.
    engine.context_tokens = 700;
    let (core, engine) = core_with(engine);
    let leo = core.start_conversation("boyfriend".into()).unwrap();
    drain(&core.send(leo.id.clone(), "first".into()).unwrap());
    drain(&core.send(leo.id.clone(), "second".into()).unwrap());

    let request = engine.last_request();
    assert!(request.instructions.contains("You are Leo"));
    // Only the new message fits after the pinned instructions.
    assert_eq!(request.turns.len(), 1);
    assert!(request.turns[0].text.ends_with("second"));
}

#[test]
fn stop_ends_the_answer_and_the_engine_and_keeps_the_text() {
    let pieces: Vec<String> = (0..1000).map(|i| format!("{i} ")).collect();
    let refs: Vec<&str> = pieces.iter().map(String::as_str).collect();
    let mut engine = FakeEngine::new(&refs);
    engine.delay = Duration::from_millis(2);
    let (core, engine) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id.clone(), "Go on".into()).unwrap();

    // Wait for some text, then stop.
    let mut seen = String::new();
    while seen.len() < 20 {
        if let Some(AssistantUpdate::Text { text }) = block_on(generation.next()) {
            seen.push_str(&text);
        }
    }
    generation.cancel();
    let (rest, _, message) = drain(&generation);
    assert_eq!(message.state, AssistantMessageState::Stopped);
    assert_eq!(message.error, None, "a stop is not a failure");
    assert_eq!(message.text, format!("{seen}{rest}"));
    assert!(message.text.len() < pieces.concat().len());
    assert_eq!(engine.computed_after_stop.load(Ordering::SeqCst), 0);

    // The stopped text is part of the history for the next answer.
    let messages = core.messages(conversation.id).unwrap();
    assert_eq!(messages[1].state, AssistantMessageState::Stopped);
    assert!(prompt_turn(messages[1].clone(), &[]).is_some());
}

#[test]
fn an_engine_that_ignores_stop_gets_nothing_more_into_the_answer() {
    let mut engine = FakeEngine::new(&["a", "b", "c", "d", "e", "f"]);
    engine.delay = Duration::from_millis(20);
    engine.ignore_stop = true;
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id, "Go".into()).unwrap();
    while !matches!(
        block_on(generation.next()),
        Some(AssistantUpdate::Text { .. })
    ) {}
    generation.cancel();
    let (_, _, message) = drain(&generation);
    assert_eq!(message.state, AssistantMessageState::Stopped);
    assert!(message.text.len() < 6, "{:?}", message.text);
}

#[test]
fn one_answer_at_a_time() {
    let mut engine = FakeEngine::new(&["slow", " answer"]);
    engine.delay = Duration::from_millis(100);
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id.clone(), "One".into()).unwrap();
    assert_eq!(
        core.send(conversation.id.clone(), "Two".into())
            .unwrap_err(),
        AssistantError::Busy
    );
    let current = core.current_generation().expect("still writing");
    assert_eq!(current.message_id(), generation.message_id());
    drain(&generation);
    assert!(core.current_generation().is_none());
    drain(&core.send(conversation.id, "Two".into()).unwrap());
}

#[test]
fn a_screen_that_opens_mid_answer_catches_up_without_repeats() {
    let mut engine = FakeEngine::new(&["one ", "two ", "three ", "four"]);
    engine.delay = Duration::from_millis(30);
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id, "Count".into()).unwrap();
    std::thread::sleep(Duration::from_millis(70));
    let reopened = core.current_generation().expect("still writing");
    let mut text = reopened.catch_up();
    let (rest, _, message) = drain(&reopened);
    text.push_str(&rest);
    assert_eq!(text, "one two three four");
    assert_eq!(message.text, text);
    assert!(generation.is_finished());
}

#[test]
fn an_unavailable_model_says_why_and_saves_nothing() {
    let mut engine = FakeEngine::new(&["never"]);
    engine.available = false;
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let err = core.send(conversation.id.clone(), "Hi".into()).unwrap_err();
    assert_eq!(
        err,
        AssistantError::Unavailable {
            reason: "Turn on the fake model".into()
        }
    );
    assert!(core.messages(conversation.id).unwrap().is_empty());
    assert!(!core.engine_info().unwrap().available);
}

#[test]
fn no_engine_and_bad_input_are_refused() {
    let core = AssistantCore::in_memory();
    assert!(core.engine_info().is_none());
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    assert_eq!(
        core.send(conversation.id.clone(), "Hi".into()).unwrap_err(),
        AssistantError::NoEngine
    );
    core.set_engine(Arc::new(FakeEngine::new(&["ok"])));
    assert!(matches!(
        core.send(conversation.id.clone(), "   ".into()),
        Err(AssistantError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.send(conversation.id, "x".repeat(MAX_MESSAGE_CHARS + 1)),
        Err(AssistantError::InvalidInput { .. })
    ));
    assert!(matches!(
        core.send("missing".into(), "Hi".into()),
        Err(AssistantError::NotFound { .. })
    ));
}

#[test]
fn refusals_and_failures_are_kept_apart_and_left_out_of_the_history() {
    let mut engine = FakeEngine::new(&[]);
    engine.result = Err(AiError::Refused {
        reason: "guardrail".into(),
    });
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let (_, _, refused) = drain(&core.send(conversation.id.clone(), "Hmm".into()).unwrap());
    assert_eq!(refused.state, AssistantMessageState::Refused);
    assert_eq!(refused.error.as_deref(), Some("guardrail"));
    assert!(prompt_turn(refused, &[]).is_none());

    let mut engine = FakeEngine::new(&["half"]);
    engine.result = Err(AiError::Engine {
        reason: "boom".into(),
    });
    core.set_engine(Arc::new(engine));
    let (_, _, failed) = drain(&core.send(conversation.id, "Again".into()).unwrap());
    assert_eq!(failed.state, AssistantMessageState::Failed);
    assert_eq!(failed.text, "half");
    assert!(failed.error.unwrap().contains("boom"));
}

#[test]
fn history_survives_a_relaunch_and_a_cut_off_answer_is_marked_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("assistant.db");
    let conversation_id;
    {
        let core = AssistantCore::open(&path).unwrap();
        core.set_engine(Arc::new(FakeEngine::new(&["Hello!"])));
        let conversation = core.start_conversation("boyfriend".into()).unwrap();
        conversation_id = conversation.id.clone();
        drain(&core.send(conversation.id.clone(), "Hi Leo".into()).unwrap());
        // An answer the app never finished (killed mid-answer).
        core.inner
            .store()
            .insert_message(&AssistantMessage {
                id: "cut-off".into(),
                conversation_id: conversation.id,
                role: AssistantRole::Assistant,
                text: String::new(),
                state: AssistantMessageState::Streaming,
                error: None,
                created_at: now_ms(),
                stats: None,
            })
            .unwrap();
    }
    let core = AssistantCore::open(&path).unwrap();
    let messages = core.messages(conversation_id.clone()).unwrap();
    let texts: Vec<_> = messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts[1..3], ["Hi Leo", "Hello!"]);
    assert_eq!(messages[3].state, AssistantMessageState::Interrupted);
    assert_eq!(core.conversations().unwrap()[0].character.name, "Leo");
}

#[test]
fn deleting_a_conversation_stops_its_answer() {
    let mut engine = FakeEngine::new(&["a", "b", "c", "d"]);
    engine.delay = Duration::from_millis(50);
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id.clone(), "Go".into()).unwrap();
    core.delete_conversation(conversation.id.clone()).unwrap();
    let (_, _, message) = drain(&generation);
    assert_eq!(message.state, AssistantMessageState::Stopped);
    assert!(core.conversations().unwrap().is_empty());
    assert!(matches!(
        core.messages(conversation.id.clone()),
        Err(AssistantError::NotFound { .. })
    ));
    assert!(matches!(
        core.delete_conversation(conversation.id),
        Err(AssistantError::NotFound { .. })
    ));
}

#[test]
fn shutdown_stops_the_answer_and_refuses_new_ones() {
    let mut engine = FakeEngine::new(&["a", "b", "c", "d"]);
    engine.delay = Duration::from_millis(50);
    let (core, _) = core_with(engine);
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id.clone(), "Go".into()).unwrap();
    core.shutdown();
    let (_, _, message) = drain(&generation);
    assert_eq!(message.state, AssistantMessageState::Stopped);
    assert_eq!(
        core.send(conversation.id, "Again".into()).unwrap_err(),
        AssistantError::ShutDown
    );
}

#[test]
fn the_engine_runs_off_the_calling_thread() {
    struct ThreadEngine(Mutex<Option<mpsc::Sender<std::thread::ThreadId>>>);
    #[async_trait::async_trait]
    impl LocalModelEngine for ThreadEngine {
        fn describe(&self) -> EngineInfo {
            FakeEngine::new(&[]).describe()
        }
        async fn generate(&self, _: GenerateRequest, _: Arc<TokenSink>) -> Result<(), AiError> {
            if let Some(tx) = self.0.lock().unwrap().take() {
                tx.send(std::thread::current().id()).unwrap();
            }
            Ok(())
        }
    }
    let (tx, rx) = mpsc::channel();
    let core = AssistantCore::in_memory();
    core.set_engine(Arc::new(ThreadEngine(Mutex::new(Some(tx)))));
    let conversation = core.start_conversation(PLAIN.into()).unwrap();
    let generation = core.send(conversation.id, "Hi".into()).unwrap();
    let engine_thread = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_ne!(engine_thread, std::thread::current().id());
    let (_, _, message) = drain(&generation);
    assert_eq!(message.state, AssistantMessageState::Done);
}

#[test]
fn an_answer_ends_where_the_model_starts_writing_the_users_side() {
    // What Foundation Models did on Cati (L1 phone run, 2026-11-08).
    let (core, engine) = core_with(FakeEngine::new(&[
        "That's so cool! ",
        "🤯 I'm so impressed!",
        "\n\nYo",
        "u: Hmm I'm learning SwiftUI",
        "\nUser: It's intuitive",
    ]));
    let mia = core.start_conversation("girlfriend".into()).unwrap();
    let generation = core
        .send(mia.id.clone(), "I build iOS tools on Linux".into())
        .unwrap();
    let (_, _, message) = drain(&generation);
    assert_eq!(message.text, "That's so cool! 🤯 I'm so impressed!");
    assert_eq!(
        message.state,
        AssistantMessageState::Done,
        "complete, not stopped"
    );
    assert_eq!(
        message.stats.unwrap().pieces,
        4,
        "the engine was told to stop"
    );

    // The next request carries only the real answer.
    drain(&core.send(mia.id, "Thanks!".into()).unwrap());
    let request = engine.last_request();
    assert_eq!(
        request.turns[2].text,
        "That's so cool! 🤯 I'm so impressed!"
    );
}

#[test]
fn turn_markers_are_found_only_at_the_start_of_a_later_line() {
    let markers = characters::find("girlfriend").turn_markers();
    let end = |t: &str| characters::end_of_turn(t, &markers);
    assert_eq!(end("Hi!\nYou: hey"), Some(3));
    assert_eq!(end("Hi!  \n\n  **User:** hey"), Some(3));
    assert_eq!(end("Hi!\nmia: again"), Some(3));
    assert_eq!(end("You: is how it starts"), None, "only a later line");
    assert_eq!(end("Tell me what you: think"), None);
    assert_eq!(end("Line one\nLine two"), None);
}

#[test]
fn saved_answers_with_invented_turns_are_cleaned_for_the_model() {
    let markers = characters::find("girlfriend").turn_markers();
    let saved = AssistantMessage {
        id: "m".into(),
        conversation_id: "c".into(),
        role: AssistantRole::Assistant,
        text: "So cool!\nYou: I'm learning SwiftUI\nUser: Haha".into(),
        state: AssistantMessageState::Done,
        error: None,
        created_at: 0,
        stats: None,
    };
    assert_eq!(prompt_turn(saved, &markers).unwrap().text, "So cool!");
}
