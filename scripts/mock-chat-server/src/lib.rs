//! In-memory chat server for the Chat example. Demo only: no auth, no
//! persistence, three hard-coded rooms. It serves the wire protocol in
//! `docs/chat-demo.md` and behaves like `chat_core::InMemoryRepo`:
//!
//! - `GET  /rooms`                          → `[{"id","title","last_message_at"}]`
//! - `GET  /rooms/{id}/messages?since=ms`   → messages with `sent_at > since`, oldest first
//! - `POST /rooms/{id}/messages`            ← `{"id"?, "author", "body"}`; `201` new, `200` if the id exists
//! - `GET  /rooms/{id}/stream`              → `text/event-stream`: `event: message`, `event: room`, `:ping`
//!
//! `sent_at` is server time in Unix ms, strictly increasing per room. A
//! stream subscriber that falls too far behind is disconnected; the client
//! reconnects and catches up with `since`.
//!
//! Keep it a single, small, in-memory file. If it needs persistence or auth,
//! the demo needs a real backend instead.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// The rooms every server starts with: `(id, title)`.
pub const ROOMS: &[(&str, &str)] = &[
    ("general", "General"),
    ("random", "Random"),
    ("forge", "Forge dev"),
];

/// Default heartbeat interval on idle streams.
pub const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(15);

/// Events buffered per stream subscriber before it is disconnected.
const STREAM_BUFFER: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireMessage {
    pub id: String,
    pub conversation_id: String,
    pub author: String,
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
    author: String,
    body: String,
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

struct Room {
    title: String,
    messages: Vec<WireMessage>,
    events: broadcast::Sender<StreamEvent>,
}

impl Room {
    fn wire(&self, id: &str) -> WireRoom {
        WireRoom {
            id: id.to_string(),
            title: self.title.clone(),
            last_message_at: self.messages.last().map(|m| m.sent_at),
        }
    }
}

struct Inner {
    order: Vec<String>,
    rooms: HashMap<String, Room>,
    next_server_id: u64,
}

/// Shared server state. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Mutex<Inner>>,
    heartbeat: Duration,
}

impl AppState {
    pub fn new(heartbeat: Duration) -> Self {
        let rooms = ROOMS
            .iter()
            .map(|(id, title)| {
                let room = Room {
                    title: (*title).to_string(),
                    messages: Vec::new(),
                    events: broadcast::channel(STREAM_BUFFER).0,
                };
                ((*id).to_string(), room)
            })
            .collect();
        Self {
            inner: Arc::new(Mutex::new(Inner {
                order: ROOMS.iter().map(|(id, _)| (*id).to_string()).collect(),
                rooms,
                next_server_id: 1,
            })),
            heartbeat,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn not_found(id: &str) -> Response {
    (StatusCode::NOT_FOUND, format!("no room {id:?}")).into_response()
}

async fn list_rooms(State(state): State<AppState>) -> Json<Vec<WireRoom>> {
    let inner = state.lock();
    Json(
        inner
            .order
            .iter()
            .map(|id| inner.rooms[id].wire(id))
            .collect(),
    )
}

async fn load_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<SinceQuery>,
) -> Response {
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
    Path(id): Path<String>,
    Json(post): Json<PostBody>,
) -> Response {
    let body = post.body.trim().to_string();
    let author = post.author.trim().to_string();
    if body.is_empty() || author.is_empty() {
        return (StatusCode::BAD_REQUEST, "author and body must not be empty").into_response();
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
        body,
        sent_at: now_ms().max(last.saturating_add(1)),
    };
    room.messages.push(message.clone());
    // No subscribers is fine: `send` only fails when nobody listens.
    let _ = room.events.send(StreamEvent::Message(message.clone()));
    let _ = room.events.send(StreamEvent::Room(room.wire(&id)));
    if used_server_id {
        inner.next_server_id += 1;
    }
    (StatusCode::CREATED, Json(message)).into_response()
}

async fn stream_room(State(state): State<AppState>, Path(id): Path<String>) -> Response {
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
        .route("/rooms", get(list_rooms))
        .route(
            "/rooms/{id}/messages",
            get(load_messages).post(post_message),
        )
        .route("/rooms/{id}/stream", get(stream_room))
        .with_state(state)
}
