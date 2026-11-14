//! `native_services!(ai)`: the host's model (Swift in an app) seen from
//! Rust, against a fake host.

use std::sync::{Arc, Mutex};

use native::ai::{
    AiModel, AiModelError, AiModelInfo, AiRequest, AiRole, AiService, ChatRequest, Collect,
    HostModel, LanguageModel, TextOut, TextSink,
};

uniffi::setup_scaffolding!("ai_test");
forge_services::native_services!(ai);

/// Answers like the Swift adapter: pushes words until told to stop.
struct FakeHost {
    words: Vec<&'static str>,
    error: Option<AiModelError>,
    seen: Mutex<Vec<AiRequest>>,
}

impl FakeHost {
    fn new(words: &[&'static str]) -> Arc<Self> {
        Arc::new(Self {
            words: words.to_vec(),
            error: None,
            seen: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl AiModel for FakeHost {
    fn describe(&self) -> AiModelInfo {
        AiModelInfo {
            id: "fake".into(),
            name: "Fake".into(),
            available: true,
            unavailable_reason: None,
            context_tokens: 4096,
        }
    }

    async fn generate(&self, request: AiRequest, sink: Arc<TextSink>) -> Result<(), AiModelError> {
        self.seen.lock().unwrap().push(request);
        if let Some(err) = &self.error {
            return Err(err.clone());
        }
        for word in &self.words {
            if !sink.push(word.to_string()) {
                return Ok(());
            }
        }
        Ok(())
    }
}

#[test]
fn the_hosts_model_answers_a_chat_request() {
    let host = FakeHost::new(&["Hello", " there."]);
    let model = HostModel(host.clone());
    assert_eq!(model.describe().id, "fake");
    let out = Arc::new(Collect::new());
    model
        .generate(ChatRequest::new("Be brief.", "Hi", 20), out.clone())
        .unwrap();
    assert_eq!(out.text(), "Hello there.");
    let seen = host.seen.lock().unwrap();
    assert_eq!(seen[0].messages[0].role, AiRole::System);
    assert_eq!(seen[0].messages[1].content, "Hi");
    assert_eq!(seen[0].max_tokens, 20);
}

/// Takes one piece, then says stop.
struct OnePiece(Mutex<Vec<String>>);

impl TextOut for OnePiece {
    fn push(&self, delta: &str) -> bool {
        self.0.lock().unwrap().push(delta.to_string());
        false
    }

    fn is_stopped(&self) -> bool {
        !self.0.lock().unwrap().is_empty()
    }
}

#[test]
fn stopping_reaches_the_host() {
    let host = FakeHost::new(&["one ", "two ", "three"]);
    let out = Arc::new(OnePiece(Mutex::new(Vec::new())));
    HostModel(host)
        .generate(ChatRequest::new("", "Hi", 20), out.clone())
        .unwrap();
    assert_eq!(*out.0.lock().unwrap(), vec!["one "]);
}

#[test]
fn host_errors_become_ai_errors() {
    let host = Arc::new(FakeHost {
        words: Vec::new(),
        error: Some(AiModelError::Busy {
            reason: "background".into(),
        }),
        seen: Mutex::new(Vec::new()),
    });
    let err = HostModel(host)
        .generate(ChatRequest::new("", "Hi", 20), Arc::new(Collect::new()))
        .unwrap_err();
    assert_eq!(err.to_string(), "busy: background");
}

#[test]
fn the_installed_model_is_found() {
    AiService::uninstall();
    assert!(AiService::installed().is_err());
    native::ai::install_native_ai(FakeHost::new(&["ok"]));
    let model = AiService::installed().unwrap().model();
    let answer = native::ai::ask(&*model, ChatRequest::new("", "Hi", 5), || false).unwrap();
    assert_eq!(answer, "ok");
    AiService::uninstall();
}
