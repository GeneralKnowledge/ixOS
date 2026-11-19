//! Asking a trusted device's language model over `relay/device/1`: devices
//! on one machine, over localhost, with scripted models and a fake
//! OpenAI-compatible server, as `relay serve-ai` and `relay ask` use them.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forge_services::ai::openai::OpenAiServer;
use forge_services::ai::{AiError, ChatRequest, Collect, LanguageModel, ScriptedModel, TextOut};
use std::path::Path;

use relay_core::{CodeSend, NearbyDevice, RelayCore, RelayError};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let core = RelayCore::new(dir.join(name).to_string_lossy().into_owned(), name.into()).unwrap();
    core.receive_into(
        dir.join(name)
            .join("Received")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
    core
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `from` sends a file to `to` with a word code, and both trust each other
/// after it, as the pairing policy offers. Returns `from`'s endpoint id.
fn trust(dir: &Path, from: &RelayCore, to: &RelayCore) -> String {
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    from.send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(from.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = from.code_send_status()
    else {
        unreachable!()
    };
    block_on(to.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id)).unwrap();
    eventually("the send to end", || {
        matches!(from.code_send_status(), CodeSend::Sent { .. })
    });
    eventually("the sender's trust offer", || from.trust_offer().is_some());
    let on_to = to.trust_offer().expect("the receiver's trust offer");
    let on_from = from.trust_offer().unwrap();
    block_on(to.trust_device(on_to.endpoint_id.clone())).unwrap();
    block_on(from.trust_device(on_from.endpoint_id)).unwrap();
    on_to.endpoint_id
}

/// Tells `core` where `other` is, as Bonjour would.
fn sees(core: &RelayCore, other: &RelayCore) {
    let service = block_on(other.device_service()).unwrap();
    core.devices_nearby(vec![NearbyDevice {
        endpoint_id: service.endpoint_id,
        addresses: vec![format!("127.0.0.1:{}", service.port)],
    }]);
}

/// A laptop and a phone that trust each other, the phone knowing where the
/// laptop is. Returns the phone, the laptop, and the laptop's endpoint id.
fn paired(dir: &tempfile::TempDir) -> (Arc<RelayCore>, Arc<RelayCore>, String) {
    let laptop = device(dir.path(), "laptop");
    let phone = device(dir.path(), "phone");
    let id = trust(dir.path(), &laptop, &phone);
    sees(&phone, &laptop);
    (phone, laptop, id)
}

fn ask(
    phone: &RelayCore,
    laptop: &str,
    request: ChatRequest,
    out: &dyn TextOut,
) -> Result<Result<String, AiError>, RelayError> {
    phone.ask(laptop, request, out)
}

#[test]
fn a_trusted_device_answers_with_its_model() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, at) = paired(&dir);
    let model = Arc::new(ScriptedModel::new(["The night train leaves at nine."]));
    laptop.answer_asks(Some(model.clone()));

    let out = Collect::new();
    let request = ChatRequest::new("Be brief.", "When does the train leave?", 64);
    let answer = ask(&phone, &at, request.clone(), &out).unwrap();
    assert_eq!(answer, Ok("scripted".to_string()));
    assert_eq!(out.text(), "The night train leaves at nine.");
    // The laptop's model got the request exactly as the phone sent it.
    assert_eq!(model.requests(), vec![request]);
}

#[test]
fn a_device_without_a_model_declines() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, at) = paired(&dir);
    let err = ask(&phone, &at, ChatRequest::new("", "Hi", 8), &Collect::new()).unwrap_err();
    assert!(matches!(err, RelayError::Declined { .. }), "{err:?}");
    // Asking stops when the model is taken away again.
    laptop.answer_asks(Some(Arc::new(ScriptedModel::new(["ok"]))));
    assert!(ask(&phone, &at, ChatRequest::new("", "Hi", 8), &Collect::new()).is_ok());
    laptop.answer_asks(None);
    assert!(ask(&phone, &at, ChatRequest::new("", "Hi", 8), &Collect::new()).is_err());
}

#[test]
fn the_models_refusal_comes_back_as_a_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, at) = paired(&dir);
    laptop.answer_asks(Some(Arc::new(ScriptedModel::with_results([Err(
        AiError::Refused {
            reason: "no".into(),
        },
    )]))));
    let answer = ask(&phone, &at, ChatRequest::new("", "Hi", 8), &Collect::new());
    assert_eq!(
        answer.unwrap(),
        Err(AiError::Refused {
            reason: "no".into()
        })
    );
}

/// Counts what the model pushed after the asker said stop.
struct Watched {
    model: ScriptedModel,
    words_written: Arc<Mutex<u32>>,
}

impl LanguageModel for Watched {
    fn describe(&self) -> forge_services::ai::ModelInfo {
        self.model.describe()
    }

    fn generate(&self, request: ChatRequest, out: Arc<dyn TextOut>) -> Result<(), AiError> {
        struct Count(Arc<dyn TextOut>, Arc<Mutex<u32>>);
        impl TextOut for Count {
            fn push(&self, delta: &str) -> bool {
                *self.1.lock().unwrap() += 1;
                self.0.push(delta)
            }
            fn is_stopped(&self) -> bool {
                self.0.is_stopped()
            }
        }
        self.model.generate(
            request,
            Arc::new(Count(out, Arc::clone(&self.words_written))),
        )
    }
}

#[test]
fn stopping_stops_the_other_devices_model() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, at) = paired(&dir);
    let long = vec!["word"; 200].join(" ");
    let written = Arc::new(Mutex::new(0));
    laptop.answer_asks(Some(Arc::new(Watched {
        model: ScriptedModel::new([long]).pause(Duration::from_millis(10)),
        words_written: Arc::clone(&written),
    })));
    // Stop as soon as some text has arrived.
    let seen = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&seen);
    let out = Collect::new()
        .listen(move |_| flag.store(true, Ordering::SeqCst))
        .stop_when(move || seen.load(Ordering::SeqCst));
    let started = Instant::now();
    let answer = ask(&phone, &at, ChatRequest::new("", "Go on", 400), &out).unwrap();
    assert_eq!(answer, Err(AiError::Stopped));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    // The laptop's model stopped long before its 200 words.
    let n = *written.lock().unwrap();
    assert!(n < 100, "the model wrote {n} words after being stopped");
}

#[test]
fn one_ask_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, at) = paired(&dir);
    let tablet = device(dir.path(), "tablet");
    trust(dir.path(), &laptop, &tablet);
    sees(&tablet, &laptop);
    let slow = vec!["slow"; 40].join(" ");
    laptop.answer_asks(Some(Arc::new(
        ScriptedModel::new([slow]).pause(Duration::from_millis(20)),
    )));
    let first = std::thread::spawn({
        let (phone, at) = (Arc::clone(&phone), at.clone());
        move || {
            ask(
                &phone,
                &at,
                ChatRequest::new("", "First", 64),
                &Collect::new(),
            )
        }
    });
    std::thread::sleep(Duration::from_millis(200));
    let second = ask(
        &tablet,
        &at,
        ChatRequest::new("", "Second", 8),
        &Collect::new(),
    );
    assert!(
        matches!(second, Ok(Err(AiError::Busy { .. }))),
        "{second:?}"
    );
    assert!(matches!(first.join().unwrap(), Ok(Ok(_))));
}

/// An OpenAI-compatible server on localhost that streams `words`, and
/// keeps each request body.
fn fake_openai_server(words: &'static [&'static str]) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&bodies);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            kept.lock().unwrap().push(String::from_utf8(body).unwrap());
            let mut events = String::new();
            for word in words {
                let chunk = serde_json::json!({"choices": [{"delta": {"content": word}}]});
                events.push_str(&format!("data: {chunk}\n\n"));
            }
            events.push_str("data: [DONE]\n\n");
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{events}",
                events.len()
            );
        }
    });
    (url, bodies)
}

#[test]
fn serve_ai_answers_with_an_openai_compatible_server() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, at) = paired(&dir);
    let (url, bodies) = fake_openai_server(&["Nine", " o'clock", "."]);
    let client = Arc::new(forge_net::UreqClient::new());
    let server = OpenAiServer::new(client, url).model("qwen3:8b");
    laptop.answer_asks(Some(Arc::new(server)));

    let out = Collect::new();
    let request = ChatRequest::new("Be brief.", "When does the train leave?", 64);
    let answer = ask(&phone, &at, request, &out).unwrap();
    assert_eq!(answer, Ok("qwen3:8b".to_string()));
    assert_eq!(out.text(), "Nine o'clock.");
    let sent: serde_json::Value = serde_json::from_str(&bodies.lock().unwrap()[0]).unwrap();
    assert_eq!(sent["model"], "qwen3:8b");
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["messages"][0]["role"], "system");
    assert_eq!(sent["messages"][1]["content"], "When does the train leave?");
}
