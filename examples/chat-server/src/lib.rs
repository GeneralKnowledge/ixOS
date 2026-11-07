//! The chat server for the Chat example: a small, real server you run on
//! your own computer. It keeps accounts and messages on disk, so a phone,
//! the `chat-cli` and the tests all talk to the same thing a real app would.
//! It serves the wire protocol in `docs/chat-demo.md` and behaves like
//! `chat_core::InMemoryRepo`:
//!
//! - `POST /accounts`                       ← `{"display_name"}` → `201` `{"user_id","display_name","token"}`
//! - `GET  /me`                             → `{"user_id","display_name"}` for the token
//! - `GET  /rooms`                          → `[{"id","title","last_message_at"}]`
//! - `GET  /rooms/{id}/messages?since=ms`   → messages with `sent_at > since`, oldest first
//! - `POST /rooms/{id}/messages`            ← `{"id"?, "body"}`; `201` new, `200` if the id exists
//! - `GET  /rooms/{id}/stream`              → `text/event-stream`: `event: message`, `event: room`, `:ping`
//!
//! Every call except `POST /accounts` needs `Authorization: Bearer <token>`;
//! the author of a message is the token's account. A missing or unknown
//! token is `401`. [`AppState::allow_anonymous`] also accepts requests
//! without a token, where the author is whatever the body's `author` says:
//! that is how the frozen Android example and `curl` post.
//!
//! `sent_at` is server time in Unix ms, strictly increasing per room. A
//! stream subscriber that falls too far behind is disconnected; the client
//! reconnects and catches up with `since`.
//!
//! Storage is one JSON file, rewritten (atomically) after every change. It
//! holds the account tokens, so it is created readable by its owner only.
//! This is a server for one person's devices on a home network: there is no
//! TLS (put it behind a TLS proxy for anything else), no rate limiting and
//! no account recovery.

use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Write;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// The rooms a new server starts with: `(id, title)`.
pub const ROOMS: &[(&str, &str)] = &[
    ("general", "General"),
    ("random", "Random"),
    ("forge", "Forge dev"),
];

/// Default heartbeat interval on idle streams.
pub const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(15);

/// Longest display name, in characters (chat-core checks the same).
pub const MAX_DISPLAY_NAME_CHARS: usize = 40;

/// Longest message body, in characters (chat-core checks the same).
pub const MAX_BODY_CHARS: usize = 4000;

/// Version of the data file. Bump it, and migrate in [`load`], when the
/// stored shape changes.
pub const DATA_VERSION: u32 = 1;

/// Events buffered per stream subscriber before it is disconnected.
const STREAM_BUFFER: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireMessage {
    pub id: String,
    pub conversation_id: String,
    pub author: String,
    /// The author's account id; empty for anonymous posts.
    #[serde(default)]
    pub author_id: String,
    pub body: String,
    pub sent_at: i64,
}

#[derive(Clone, Debug, Serialize)]
struct WireRoom {
    id: String,
    title: String,
    last_message_at: Option<i64>,
}

#[derive(Deserialize)]
struct PostBody {
    id: Option<String>,
    /// Only used for anonymous posts; an account's posts carry its name.
    #[serde(default)]
    author: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct RegisterBody {
    display_name: String,
}

#[derive(Serialize)]
struct WireAccount {
    user_id: String,
    display_name: String,
    token: String,
}

#[derive(Serialize)]
struct WireIdentity {
    user_id: String,
    display_name: String,
}

#[derive(Deserialize)]
struct SinceQuery {
    since: Option<i64>,
}

#[derive(Clone)]
enum StreamEvent {
    Message(WireMessage),
    Room(WireRoom),
}

/// An account, as stored.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct User {
    id: String,
    display_name: String,
    token: String,
    created_at: i64,
}

struct Room {
    title: String,
    messages: Vec<WireMessage>,
    events: broadcast::Sender<StreamEvent>,
}

impl Room {
    fn new(title: String, messages: Vec<WireMessage>) -> Self {
        Self {
            title,
            messages,
            events: broadcast::channel(STREAM_BUFFER).0,
        }
    }

    fn wire(&self, id: &str) -> WireRoom {
        WireRoom {
            id: id.to_string(),
            title: self.title.clone(),
            last_message_at: self.messages.last().map(|m| m.sent_at),
        }
    }
}

/// The data file.
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    users: Vec<User>,
    rooms: Vec<StoredRoom>,
    next_server_id: u64,
}

#[derive(Serialize, Deserialize)]
struct StoredRoom {
    id: String,
    title: String,
    messages: Vec<WireMessage>,
}

struct Inner {
    order: Vec<String>,
    rooms: HashMap<String, Room>,
    users: Vec<User>,
    next_server_id: u64,
    /// Where to save; `None` keeps everything in memory.
    data_file: Option<PathBuf>,
}

impl Inner {
    fn user_by_token(&self, token: &str) -> Option<&User> {
        self.users
            .iter()
            .find(|u| constant_time_eq(&u.token, token))
    }

    fn stored(&self) -> Stored {
        Stored {
            version: DATA_VERSION,
            users: self.users.clone(),
            rooms: self
                .order
                .iter()
                .map(|id| StoredRoom {
                    id: id.clone(),
                    title: self.rooms[id].title.clone(),
                    messages: self.rooms[id].messages.clone(),
                })
                .collect(),
            next_server_id: self.next_server_id,
        }
    }

    /// Writes the data file, if there is one. Called under the lock after
    /// every change, so the file always matches what clients were told.
    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.data_file else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(&self.stored()).map_err(std::io::Error::other)?;
        write_private_atomically(path, &json)
    }
}

/// Shared server state. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Mutex<Inner>>,
    heartbeat: Duration,
    allow_anonymous: bool,
}

/// Why the data file could not be used.
#[derive(Debug)]
pub enum DataError {
    Io(PathBuf, std::io::Error),
    Corrupt(PathBuf, String),
    NewerVersion(PathBuf, u32),
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, err) => write!(f, "{}: {err}", path.display()),
            Self::Corrupt(path, why) => write!(
                f,
                "{} is not a chat server data file ({why}); move it away to start fresh",
                path.display()
            ),
            Self::NewerVersion(path, version) => write!(
                f,
                "{} was written by a newer chat server (data version {version}, this one reads {DATA_VERSION})",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DataError {}

impl AppState {
    /// An in-memory server with the default rooms (tests, `--in-memory`).
    pub fn new(heartbeat: Duration) -> Self {
        Self::from_stored(fresh(), None, heartbeat)
    }

    /// A server that keeps its data in `data_file`, loading it if it
    /// exists and creating it (and its directory) if not.
    pub fn open(data_file: impl Into<PathBuf>, heartbeat: Duration) -> Result<Self, DataError> {
        let data_file = data_file.into();
        let stored = load(&data_file)?;
        let state = Self::from_stored(stored, Some(data_file.clone()), heartbeat);
        state
            .lock()
            .save()
            .map_err(|err| DataError::Io(data_file, err))?;
        Ok(state)
    }

    /// Also accept requests without a token (the author is the body's
    /// `author`). Off by default.
    pub fn allow_anonymous(mut self, allow: bool) -> Self {
        self.allow_anonymous = allow;
        self
    }

    /// Number of accounts.
    pub fn account_count(&self) -> usize {
        self.lock().users.len()
    }

    fn from_stored(stored: Stored, data_file: Option<PathBuf>, heartbeat: Duration) -> Self {
        let order = stored.rooms.iter().map(|r| r.id.clone()).collect();
        let rooms = stored
            .rooms
            .into_iter()
            .map(|r| (r.id, Room::new(r.title, r.messages)))
            .collect();
        Self {
            inner: Arc::new(Mutex::new(Inner {
                order,
                rooms,
                users: stored.users,
                next_server_id: stored.next_server_id,
                data_file,
            })),
            heartbeat,
            allow_anonymous: false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn fresh() -> Stored {
    Stored {
        version: DATA_VERSION,
        users: Vec::new(),
        rooms: ROOMS
            .iter()
            .map(|(id, title)| StoredRoom {
                id: (*id).into(),
                title: (*title).into(),
                messages: Vec::new(),
            })
            .collect(),
        next_server_id: 1,
    }
}

fn load(path: &FsPath) -> Result<Stored, DataError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(fresh()),
        Err(err) => return Err(DataError::Io(path.into(), err)),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| DataError::Corrupt(path.into(), err.to_string()))?;
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| DataError::Corrupt(path.into(), "no version".into()))?;
    if version > u64::from(DATA_VERSION) {
        return Err(DataError::NewerVersion(path.into(), version as u32));
    }
    serde_json::from_value(value).map_err(|err| DataError::Corrupt(path.into(), err.to_string()))
}

/// Writes `bytes` to a temporary file next to `path` (owner-only on Unix),
/// then renames it over `path`, so a crash never leaves half a file.
fn write_private_atomically(path: &FsPath, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A random hex string of `bytes` bytes from the OS's generator.
fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(std::io::Error::other)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compares tokens without stopping at the first different byte.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn not_found(id: &str) -> Response {
    (StatusCode::NOT_FOUND, format!("no room {id:?}")).into_response()
}

fn unauthorized(why: &str) -> Response {
    (StatusCode::UNAUTHORIZED, why.to_string()).into_response()
}

fn storage_failed(err: std::io::Error) -> Response {
    eprintln!("error: could not save the data file: {err}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "the server could not save; see its log",
    )
        .into_response()
}

/// Who is calling.
enum Caller {
    User { id: String, display_name: String },
    Anonymous,
}

/// Resolves the `Authorization` header. A token that is present must be
/// valid; no token is only allowed with [`AppState::allow_anonymous`].
/// `Err` is why the caller is refused (answered with 401).
fn caller(state: &AppState, headers: &HeaderMap) -> Result<Caller, &'static str> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match header {
        Some(value) => {
            let token = value
                .strip_prefix("Bearer ")
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or("expected Authorization: Bearer <token>")?;
            let inner = state.lock();
            let user = inner
                .user_by_token(token)
                .ok_or("unknown token; create an account again")?;
            Ok(Caller::User {
                id: user.id.clone(),
                display_name: user.display_name.clone(),
            })
        }
        None if state.allow_anonymous => Ok(Caller::Anonymous),
        None => Err("this server needs an account token"),
    }
}

fn clean_display_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("display_name must not be empty");
    }
    if name.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err("display_name is too long");
    }
    if name.chars().any(char::is_control) {
        return Err("display_name contains control characters");
    }
    Ok(name.to_string())
}

async fn register(State(state): State<AppState>, Json(body): Json<RegisterBody>) -> Response {
    let display_name = match clean_display_name(&body.display_name) {
        Ok(name) => name,
        Err(why) => return (StatusCode::BAD_REQUEST, why).into_response(),
    };
    let (id, token) = match (random_hex(8), random_hex(32)) {
        (Ok(id), Ok(token)) => (format!("u-{id}"), token),
        (Err(err), _) | (_, Err(err)) => {
            eprintln!("error: no randomness for a token: {err}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let user = User {
        id: id.clone(),
        display_name: display_name.clone(),
        token: token.clone(),
        created_at: now_ms(),
    };
    let mut inner = state.lock();
    inner.users.push(user);
    if let Err(err) = inner.save() {
        inner.users.pop();
        return storage_failed(err);
    }
    (
        StatusCode::CREATED,
        Json(WireAccount {
            user_id: id,
            display_name,
            token,
        }),
    )
        .into_response()
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match caller(&state, &headers) {
        Ok(Caller::User { id, display_name }) => Json(WireIdentity {
            user_id: id,
            display_name,
        })
        .into_response(),
        Ok(Caller::Anonymous) => unauthorized("no token"),
        Err(why) => unauthorized(why),
    }
}

async fn list_rooms(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(why) = caller(&state, &headers) {
        return unauthorized(why);
    }
    let inner = state.lock();
    Json(
        inner
            .order
            .iter()
            .map(|id| inner.rooms[id].wire(id))
            .collect::<Vec<_>>(),
    )
    .into_response()
}

async fn load_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<SinceQuery>,
) -> Response {
    if let Err(why) = caller(&state, &headers) {
        return unauthorized(why);
    }
    let inner = state.lock();
    let Some(room) = inner.rooms.get(&id) else {
        return not_found(&id);
    };
    let messages: Vec<WireMessage> = room
        .messages
        .iter()
        .filter(|m| query.since.is_none_or(|since| m.sent_at > since))
        .cloned()
        .collect();
    Json(messages).into_response()
}

async fn post_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(post): Json<PostBody>,
) -> Response {
    let (author, author_id) = match caller(&state, &headers) {
        Ok(Caller::User { id, display_name }) => (display_name, id),
        Ok(Caller::Anonymous) => (
            post.author
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
            String::new(),
        ),
        Err(why) => return unauthorized(why),
    };
    let body = post.body.trim().to_string();
    if body.is_empty() || author.is_empty() {
        return (StatusCode::BAD_REQUEST, "author and body must not be empty").into_response();
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return (StatusCode::BAD_REQUEST, "body is too long").into_response();
    }
    let mut inner = state.lock();
    let server_id = inner.next_server_id;
    let Some(room) = inner.rooms.get_mut(&id) else {
        return not_found(&id);
    };
    if let Some(existing) = post
        .id
        .as_ref()
        .and_then(|mid| room.messages.iter().find(|m| &m.id == mid))
    {
        return (StatusCode::OK, Json(existing.clone())).into_response();
    }
    let last = room.messages.last().map_or(i64::MIN, |m| m.sent_at);
    let used_server_id = post.id.is_none();
    let message = WireMessage {
        id: post.id.unwrap_or_else(|| format!("srv-{server_id}")),
        conversation_id: id.clone(),
        author,
        author_id,
        body,
        sent_at: now_ms().max(last.saturating_add(1)),
    };
    room.messages.push(message.clone());
    if used_server_id {
        inner.next_server_id += 1;
    }
    // Saved before anyone is told, so a message a client saw survives a
    // server restart.
    if let Err(err) = inner.save() {
        if let Some(room) = inner.rooms.get_mut(&id) {
            room.messages.pop();
        }
        if used_server_id {
            inner.next_server_id -= 1;
        }
        return storage_failed(err);
    }
    let room = &inner.rooms[&id];
    // No subscribers is fine: `send` only fails when nobody listens.
    let _ = room.events.send(StreamEvent::Message(message.clone()));
    let _ = room.events.send(StreamEvent::Room(room.wire(&id)));
    (StatusCode::CREATED, Json(message)).into_response()
}

async fn stream_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(why) = caller(&state, &headers) {
        return unauthorized(why);
    }
    let receiver = {
        let inner = state.lock();
        match inner.rooms.get(&id) {
            Some(room) => room.events.subscribe(),
            None => return not_found(&id),
        }
    };
    Sse::new(events(receiver))
        .keep_alive(KeepAlive::new().interval(state.heartbeat).text("ping"))
        .into_response()
}

/// Turns a room's broadcast into SSE events. A subscriber that lagged is
/// disconnected (the stream ends) rather than silently skipping messages.
fn events(
    receiver: broadcast::Receiver<StreamEvent>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(Some(receiver), |receiver| async move {
        let mut receiver = receiver?;
        let event = match receiver.recv().await {
            Ok(StreamEvent::Message(m)) => Event::default().event("message").json_data(m),
            Ok(StreamEvent::Room(r)) => Event::default().event("room").json_data(r),
            Err(_) => return None, // lagged or closed: end the stream
        };
        let event = event.expect("wire types serialise");
        Some((Ok(event), Some(receiver)))
    })
}

/// The router, for `main` and for in-process tests.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/accounts", post(register))
        .route("/me", get(me))
        .route("/rooms", get(list_rooms))
        .route(
            "/rooms/{id}/messages",
            get(load_messages).post(post_message),
        )
        .route("/rooms/{id}/stream", get(stream_room))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_survives_a_restart_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/server.json");
        {
            let state = AppState::open(&path, DEFAULT_HEARTBEAT).unwrap();
            let mut inner = state.lock();
            inner.users.push(User {
                id: "u-1".into(),
                display_name: "Ana".into(),
                token: "secret".into(),
                created_at: 1,
            });
            inner
                .rooms
                .get_mut("general")
                .unwrap()
                .messages
                .push(WireMessage {
                    id: "m1".into(),
                    conversation_id: "general".into(),
                    author: "Ana".into(),
                    author_id: "u-1".into(),
                    body: "hi".into(),
                    sent_at: 5,
                });
            inner.save().unwrap();
        }
        let state = AppState::open(&path, DEFAULT_HEARTBEAT).unwrap();
        assert_eq!(state.account_count(), 1);
        let inner = state.lock();
        assert_eq!(inner.rooms["general"].messages[0].body, "hi");
        assert_eq!(inner.user_by_token("secret").unwrap().id, "u-1");
        assert!(inner.user_by_token("Secret").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_or_newer_data_is_refused_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            AppState::open(&path, DEFAULT_HEARTBEAT),
            Err(DataError::Corrupt(..))
        ));
        std::fs::write(&path, r#"{"version": 99}"#).unwrap();
        assert!(matches!(
            AppState::open(&path, DEFAULT_HEARTBEAT),
            Err(DataError::NewerVersion(_, 99))
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"version": 99}"#
        );
    }

    #[test]
    fn display_names_are_checked() {
        assert_eq!(clean_display_name("  Reuben ").unwrap(), "Reuben");
        assert!(clean_display_name("").is_err());
        assert!(clean_display_name(&"x".repeat(41)).is_err());
        assert!(clean_display_name("a\u{7}").is_err());
    }

    #[test]
    fn tokens_are_random_and_long() {
        let a = random_hex(32).unwrap();
        let b = random_hex(32).unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
