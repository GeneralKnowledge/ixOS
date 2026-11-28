//! Jamendo's answers kept on the phone: asked once while fresh, asked
//! again when stale, used when Jamendo can't answer, never empty ones,
//! and never the client ID on disk.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use forge_net::{HttpClient, NetError, Request, Response};
use soundscape_core::jamendo_cache::{key, Clock, JamendoAnswers, FRESH_MS};

const URL: &str =
    "https://api.jamendo.com/v3.0/tracks/?client_id=secret123&format=json&artist_id=42";
const FOUND: &str = r#"{"headers": {"status": "success"}, "results": [{"id": "1"}]}"#;
const NOTHING: &str = r#"{"headers": {"status": "success"}, "results": []}"#;
const REFUSED: &str =
    r#"{"headers": {"status": "failed", "error_message": "bad client id"}, "results": []}"#;

/// Answers with whatever it is set to, counting the asks.
struct Jamendo {
    answer: Mutex<Result<(u16, String), ()>>,
    asks: AtomicUsize,
}

impl Jamendo {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Ok((200, FOUND.into()))),
            asks: AtomicUsize::new(0),
        })
    }

    fn answers(&self, status: u16, body: &str) {
        *self.answer.lock().unwrap() = Ok((status, body.into()));
    }

    fn goes_offline(&self) {
        *self.answer.lock().unwrap() = Err(());
    }

    fn asks(&self) -> usize {
        self.asks.load(Ordering::SeqCst)
    }
}

impl HttpClient for Jamendo {
    fn send(&self, _: &Request) -> Result<Response, NetError> {
        self.asks.fetch_add(1, Ordering::SeqCst);
        match &*self.answer.lock().unwrap() {
            Ok((status, body)) => Ok(Response::new(*status, body.clone())),
            Err(()) => Err(NetError::Offline {
                reason: "no route".into(),
            }),
        }
    }
}

struct Setup {
    jamendo: Arc<Jamendo>,
    time: Arc<AtomicU64>,
    answers: JamendoAnswers,
}

fn clock(time: &Arc<AtomicU64>) -> Clock {
    let time = Arc::clone(time);
    Arc::new(move || time.load(Ordering::SeqCst))
}

fn setup(dir: Option<std::path::PathBuf>) -> Setup {
    let jamendo = Jamendo::new();
    // The real time: files on disk are swept by their age.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let time = Arc::new(AtomicU64::new(now));
    let answers = JamendoAnswers::new(jamendo.clone(), dir, clock(&time));
    Setup {
        jamendo,
        time,
        answers,
    }
}

fn ask(answers: &JamendoAnswers) -> Result<Response, NetError> {
    answers.send(&Request::get(URL))
}

fn body(response: Result<Response, NetError>) -> String {
    String::from_utf8(response.expect("an answer").body).unwrap()
}

#[test]
fn a_fresh_answer_is_not_asked_for_again() {
    let s = setup(None);
    assert_eq!(body(ask(&s.answers)), FOUND);
    assert_eq!(body(ask(&s.answers)), FOUND);
    assert_eq!(s.jamendo.asks(), 1);
}

#[test]
fn a_stale_answer_is_asked_for_again() {
    let s = setup(None);
    ask(&s.answers).unwrap();
    s.time.fetch_add(FRESH_MS, Ordering::SeqCst);
    ask(&s.answers).unwrap();
    assert_eq!(s.jamendo.asks(), 2);
}

#[test]
fn refreshing_asks_again_but_keeps_the_old_answer_for_when_jamendo_cant_answer() {
    let s = setup(None);
    ask(&s.answers).unwrap();
    // In the same millisecond as the answer was kept.
    s.answers.refresh();
    s.jamendo.goes_offline();
    assert_eq!(body(ask(&s.answers)), FOUND);
    assert_eq!(s.jamendo.asks(), 2);
}

#[test]
fn a_kept_answer_is_used_when_jamendo_is_unreachable_or_failing() {
    let s = setup(None);
    ask(&s.answers).unwrap();
    s.time.fetch_add(FRESH_MS, Ordering::SeqCst);
    s.jamendo.goes_offline();
    assert_eq!(body(ask(&s.answers)), FOUND);
    s.jamendo.answers(503, "busy");
    assert_eq!(body(ask(&s.answers)), FOUND);
    s.jamendo.answers(200, NOTHING);
    assert_eq!(body(ask(&s.answers)), FOUND);
}

#[test]
fn with_nothing_kept_a_failure_is_the_failure() {
    let s = setup(None);
    s.jamendo.goes_offline();
    assert!(matches!(ask(&s.answers), Err(NetError::Offline { .. })));
}

#[test]
fn an_empty_answer_is_never_kept() {
    let s = setup(None);
    s.jamendo.answers(200, NOTHING);
    assert_eq!(body(ask(&s.answers)), NOTHING);
    ask(&s.answers).unwrap();
    assert_eq!(s.jamendo.asks(), 2);
}

#[test]
fn a_refusal_is_not_hidden_by_a_kept_answer() {
    let s = setup(None);
    ask(&s.answers).unwrap();
    s.time.fetch_add(FRESH_MS, Ordering::SeqCst);
    s.jamendo.answers(200, REFUSED);
    assert_eq!(body(ask(&s.answers)), REFUSED);
}

#[test]
fn answers_outlive_the_app_and_never_hold_the_client_id() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(Some(dir.path().to_path_buf()));
    ask(&s.answers).unwrap();

    let again = Jamendo::new();
    again.goes_offline();
    let reopened = JamendoAnswers::new(again.clone(), Some(dir.path().into()), clock(&s.time));
    assert_eq!(body(ask(&reopened)), FOUND);
    assert_eq!(again.asks(), 0, "fresh from disk, so Jamendo isn't asked");

    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(
            !text.contains("secret123"),
            "the client ID was written: {text}"
        );
    }
}

#[test]
fn only_gets_to_jamendos_api_are_kept() {
    let s = setup(None);
    let cover = "https://usercontent.jamendo.com/?type=album&id=501";
    s.answers.send(&Request::get(cover)).unwrap();
    s.answers.send(&Request::get(cover)).unwrap();
    assert_eq!(s.jamendo.asks(), 2);
    assert_eq!(key(&Request::get(cover)), None);
    assert_eq!(
        key(&Request::get(URL)).as_deref(),
        Some("https://api.jamendo.com/v3.0/tracks/?format=json&artist_id=42")
    );
}

#[test]
fn a_cancelled_ask_is_cancelled_even_with_an_answer_kept() {
    let s = setup(None);
    ask(&s.answers).unwrap();
    let cancel = forge_net::CancelHandle::new();
    cancel.cancel();
    assert!(matches!(
        s.answers.send_cancellable(&Request::get(URL), &cancel),
        Err(NetError::Cancelled)
    ));
}

#[test]
fn an_answer_kept_after_a_refresh_stays_fresh_when_read_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let s = setup(Some(dir.path().to_path_buf()));
    s.answers.refresh();
    ask(&s.answers).unwrap();
    // Enough other answers to push it out of memory.
    for n in 0..=soundscape_core::jamendo_cache::MAX_IN_MEMORY {
        s.time.fetch_add(1, Ordering::SeqCst);
        let other = format!("{URL}&page={n}");
        s.answers.send(&Request::get(other)).unwrap();
    }
    let asks = s.jamendo.asks();
    assert_eq!(body(ask(&s.answers)), FOUND);
    assert_eq!(s.jamendo.asks(), asks, "fresh from disk, not asked again");
}
