//! [`ChatRepo`] over HTTP + Server-Sent Events, through `forge_net`.
//!
//! The wire protocol (what `scripts/mock-chat-server` serves):
//!
//! | Call | Request | Success |
//! |---|---|---|
//! | list | `GET {base}/rooms` | `200` `[Room]` |
//! | load | `GET {base}/rooms/{id}/messages[?since=ms]` | `200` `[Message]`, oldest first, `sent_at > since` |
//! | post | `POST {base}/rooms/{id}/messages` `{"id","author","body"}` | `200`/`201` `Message` (idempotent by `id`) |
//! | stream | `GET {base}/rooms/{id}/stream` | `200` `text/event-stream` |
//!
//! `Room` is `{"id","title","last_message_at"}` and `Message` is
//! `{"id","conversation_id","author","body","sent_at"}` (times in Unix ms).
//! The stream sends `event: message` / `event: room` with that JSON as
//! `data`, and `: ping` comment lines as heartbeats (the mock server sends
//! one every 15 s). Unknown event names and JSON fields are ignored. A `404`
//! on any room path is [`ChatError::NotFound`].

use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Request, Response};
use serde::{Deserialize, Serialize};

use crate::repo::{ChatRepo, OutgoingMessage, RemoteConversation, RemoteEvent, RemoteStream};
use crate::sse::{SseItem, SseReader};
use crate::types::{ChatError, Message, MessageStatus};

/// Timeout for list/load/post, and for opening a stream (not for reading it).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Error bodies longer than this are cut in `ChatError::Http::reason`.
const MAX_REASON_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
struct WireRoom {
    id: String,
    title: String,
    #[serde(default)]
    last_message_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WireMessage {
    id: String,
    conversation_id: String,
    author: String,
    body: String,
    sent_at: i64,
}

#[derive(Serialize)]
struct WirePost<'a> {
    id: &'a str,
    author: &'a str,
    body: &'a str,
}

impl From<WireRoom> for RemoteConversation {
    fn from(room: WireRoom) -> Self {
        Self {
            id: room.id,
            title: room.title,
            last_message_at: room.last_message_at,
        }
    }
}

impl From<WireMessage> for Message {
    fn from(m: WireMessage) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            author: m.author,
            body: m.body,
            sent_at: m.sent_at,
            status: MessageStatus::Sent,
        }
    }
}

/// HTTP chat backend. `base_url` is e.g. `http://10.0.2.2:8787` (the Android
/// emulator's view of the host) or `http://127.0.0.1:8787`.
pub struct HttpRepo {
    client: Arc<dyn HttpClient>,
    base_url: String,
    timeout: Duration,
}

impl HttpRepo {
    pub fn new(client: Arc<dyn HttpClient>, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// A repo over forge-net's real transport.
    #[cfg(feature = "ureq")]
    pub fn with_ureq(base_url: impl Into<String>) -> Self {
        Self::new(
            Arc::new(forge_net::UreqClient::with_user_agent(concat!(
                "forge-chat-example/",
                env!("CARGO_PKG_VERSION")
            ))),
            base_url,
        )
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn room_url(&self, conversation_id: &str, rest: &str) -> String {
        format!(
            "{}/rooms/{}{rest}",
            self.base_url,
            encode_path_segment(conversation_id)
        )
    }

    fn send(&self, request: Request, room: Option<&str>) -> Result<Response, ChatError> {
        let response = self.client.send(&request.timeout(self.timeout))?;
        check_status(response.status, &response.body, room)?;
        Ok(response)
    }
}

/// Non-2xx → typed error. A 404 on a room path is `NotFound`.
fn check_status(status: u16, body: &[u8], room: Option<&str>) -> Result<(), ChatError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    if let (404, Some(room)) = (status, room) {
        return Err(ChatError::NotFound {
            conversation_id: room.into(),
        });
    }
    let text = String::from_utf8_lossy(body);
    let mut reason: String = text.trim().chars().take(MAX_REASON_CHARS).collect();
    if reason.is_empty() {
        reason = "no response body".into();
    }
    Err(ChatError::Http { status, reason })
}

fn decode<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ChatError> {
    serde_json::from_slice(body).map_err(ChatError::protocol)
}

/// Percent-encodes everything outside RFC 3986's unreserved set.
fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl ChatRepo for HttpRepo {
    fn list_conversations(&self) -> Result<Vec<RemoteConversation>, ChatError> {
        let url = format!("{}/rooms", self.base_url);
        let response = self.send(Request::get(url).header("Accept", "application/json"), None)?;
        let rooms: Vec<WireRoom> = decode(&response.body)?;
        Ok(rooms.into_iter().map(Into::into).collect())
    }

    fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError> {
        let query = since.map(|s| format!("?since={s}")).unwrap_or_default();
        let url = self.room_url(conversation_id, &format!("/messages{query}"));
        let response = self.send(
            Request::get(url).header("Accept", "application/json"),
            Some(conversation_id),
        )?;
        let messages: Vec<WireMessage> = decode(&response.body)?;
        let mut messages: Vec<Message> = messages.into_iter().map(Into::into).collect();
        if let Some(wrong) = messages
            .iter()
            .find(|m| m.conversation_id != conversation_id)
        {
            return Err(ChatError::protocol(format!(
                "message {} belongs to {:?}, not {conversation_id:?}",
                wrong.id, wrong.conversation_id
            )));
        }
        messages.sort_by_key(|m| m.sent_at);
        Ok(messages)
    }

    fn post_message(&self, message: &OutgoingMessage) -> Result<Message, ChatError> {
        let url = self.room_url(&message.conversation_id, "/messages");
        let body = serde_json::to_vec(&WirePost {
            id: &message.id,
            author: &message.author,
            body: &message.body,
        })
        .map_err(ChatError::protocol)?;
        let response = self.send(
            Request::post(url, body)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json"),
            Some(&message.conversation_id),
        )?;
        let stored: Message = decode::<WireMessage>(&response.body)?.into();
        if stored.id != message.id {
            return Err(ChatError::protocol(format!(
                "server stored {} as {}",
                message.id, stored.id
            )));
        }
        Ok(stored)
    }

    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError> {
        let url = self.room_url(conversation_id, "/stream");
        let request = Request::get(url)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .timeout(self.timeout);
        let mut response = self.client.open_stream_cancellable(&request, cancel)?;
        if !response.is_success() {
            let mut body = Vec::new();
            let _ = std::io::Read::read_to_end(
                &mut std::io::Read::take(&mut response.body, 4096),
                &mut body,
            );
            check_status(response.status, &body, Some(conversation_id))?;
        }
        let content_type = response.header("content-type").unwrap_or_default();
        if !content_type
            .to_ascii_lowercase()
            .starts_with("text/event-stream")
        {
            return Err(ChatError::protocol(format!(
                "stream answered with content type {content_type:?}"
            )));
        }
        Ok(Box::new(SseStream {
            reader: SseReader::new(BufReader::new(response.body)),
            conversation_id: conversation_id.into(),
        }))
    }
}

struct SseStream {
    reader: SseReader<BufReader<Box<dyn std::io::Read + Send>>>,
    conversation_id: String,
}

impl SseStream {
    fn decode_event(&self, event: &str, data: &str) -> Option<Result<RemoteEvent, ChatError>> {
        match event {
            "message" => Some(
                decode::<WireMessage>(data.as_bytes())
                    .map(Message::from)
                    .and_then(|m| {
                        if m.conversation_id == self.conversation_id {
                            Ok(RemoteEvent::Message(m))
                        } else {
                            Err(ChatError::protocol(format!(
                                "stream for {:?} delivered a message for {:?}",
                                self.conversation_id, m.conversation_id
                            )))
                        }
                    }),
            ),
            "room" => Some(
                decode::<WireRoom>(data.as_bytes()).map(|r| RemoteEvent::Conversation(r.into())),
            ),
            _ => None,
        }
    }
}

impl RemoteStream for SseStream {
    fn next_event(&mut self) -> Option<Result<RemoteEvent, ChatError>> {
        match self.reader.next()? {
            Ok(SseItem::Comment(_)) => Some(Ok(RemoteEvent::Heartbeat)),
            // Unknown event names are skipped, but still count as activity:
            // report a heartbeat so the caller checks for cancellation.
            Ok(SseItem::Event(event)) => Some(
                self.decode_event(&event.event, &event.data)
                    .unwrap_or(Ok(RemoteEvent::Heartbeat)),
            ),
            Err(err) if forge_net::is_cancelled_io_error(&err) => Some(Err(ChatError::Cancelled)),
            Err(err) => Some(Err(ChatError::Network {
                reason: format!("stream broke: {err}"),
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_net::{Method, NetError, StaticClient};

    const BASE: &str = "http://chat.test";

    fn repo(client: StaticClient) -> (HttpRepo, Arc<StaticClient>) {
        let client = Arc::new(client);
        (HttpRepo::new(client.clone(), format!("{BASE}/")), client)
    }

    fn json(status: u16, body: &str) -> Result<Response, NetError> {
        Ok(Response::new(status, body))
    }

    fn sse(body: &str) -> Result<Response, NetError> {
        let mut response = Response::new(200, body);
        response.headers.push(forge_net::Header {
            name: "Content-Type".into(),
            value: "text/event-stream; charset=utf-8".into(),
        });
        Ok(response)
    }

    #[test]
    fn lists_rooms_ignoring_unknown_fields() {
        let (repo, _) = repo(StaticClient::new().route(
            Method::Get,
            format!("{BASE}/rooms"),
            json(
                200,
                r#"[{"id":"general","title":"General","last_message_at":5,"extra":true},{"id":"x","title":"X"}]"#,
            ),
        ));
        let rooms = repo.list_conversations().unwrap();
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[0].last_message_at, Some(5));
        assert_eq!(rooms[1].last_message_at, None);
    }

    #[test]
    fn load_sends_since_and_marks_messages_sent() {
        let (repo, client) = repo(StaticClient::new().route(
            Method::Get,
            format!("{BASE}/rooms/general/messages?since=10"),
            json(
                200,
                r#"[{"id":"b","conversation_id":"general","author":"ana","body":"2","sent_at":12},
                    {"id":"a","conversation_id":"general","author":"ana","body":"1","sent_at":11}]"#,
            ),
        ));
        let got = repo.load_messages("general", Some(10)).unwrap();
        assert_eq!(
            got.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(got.iter().all(|m| m.status == MessageStatus::Sent));
        assert_eq!(client.received()[0].timeout, Some(DEFAULT_TIMEOUT));
    }

    #[test]
    fn room_ids_are_percent_encoded() {
        let (repo, client) = repo(StaticClient::new());
        let _ = repo.load_messages("a b/c", None);
        assert_eq!(
            client.received()[0].url,
            format!("{BASE}/rooms/a%20b%2Fc/messages")
        );
    }

    #[test]
    fn status_codes_map_to_typed_errors() {
        let (repo, _) = repo(
            StaticClient::new()
                .route(Method::Get, format!("{BASE}/rooms"), json(503, " busy "))
                .route(
                    Method::Get,
                    format!("{BASE}/rooms/general/messages"),
                    json(200, "not json"),
                ),
        );
        assert_eq!(
            repo.list_conversations().unwrap_err(),
            ChatError::Http {
                status: 503,
                reason: "busy".into()
            }
        );
        assert!(matches!(
            repo.load_messages("general", None).unwrap_err(),
            ChatError::Protocol { .. }
        ));
        // StaticClient answers unknown routes with 404.
        assert_eq!(
            repo.load_messages("gone", None).unwrap_err(),
            ChatError::NotFound {
                conversation_id: "gone".into()
            }
        );
    }

    #[test]
    fn transport_errors_are_network_errors() {
        let (repo, _) = repo(StaticClient::new().route(
            Method::Get,
            format!("{BASE}/rooms"),
            Err(NetError::Offline {
                reason: "refused".into(),
            }),
        ));
        assert!(repo.list_conversations().unwrap_err().is_network());
    }

    #[test]
    fn post_sends_json_and_checks_the_echoed_id() {
        let reply =
            r#"{"id":"m1","conversation_id":"general","author":"me","body":"hi","sent_at":42}"#;
        let (repo, client) = repo(StaticClient::new().route(
            Method::Post,
            format!("{BASE}/rooms/general/messages"),
            json(201, reply),
        ));
        let out = OutgoingMessage {
            id: "m1".into(),
            conversation_id: "general".into(),
            author: "me".into(),
            body: "hi".into(),
        };
        let stored = repo.post_message(&out).unwrap();
        assert_eq!(stored.sent_at, 42);
        assert_eq!(stored.status, MessageStatus::Sent);
        let sent: serde_json::Value = serde_json::from_slice(&client.received()[0].body).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"id":"m1","author":"me","body":"hi"})
        );

        let other = OutgoingMessage {
            id: "m2".into(),
            ..out
        };
        assert!(matches!(
            repo.post_message(&other).unwrap_err(),
            ChatError::Protocol { .. }
        ));
    }

    #[test]
    fn stream_decodes_messages_rooms_and_heartbeats() {
        let body = concat!(
            ": ping\n\n",
            "event: message\n",
            "data: {\"id\":\"a\",\"conversation_id\":\"general\",\"author\":\"ana\",\"body\":\"hi\",\"sent_at\":1}\n\n",
            "event: typing\ndata: {}\n\n",
            "event: room\ndata: {\"id\":\"general\",\"title\":\"General!\",\"last_message_at\":1}\n\n",
        );
        let (repo, client) = repo(StaticClient::new().route(
            Method::Get,
            format!("{BASE}/rooms/general/stream"),
            sse(body),
        ));
        let mut stream = repo.open_stream("general", &CancelHandle::new()).unwrap();
        assert_eq!(stream.next_event(), Some(Ok(RemoteEvent::Heartbeat)));
        let Some(Ok(RemoteEvent::Message(m))) = stream.next_event() else {
            panic!("expected a message")
        };
        assert_eq!((m.id.as_str(), m.status), ("a", MessageStatus::Sent));
        assert_eq!(stream.next_event(), Some(Ok(RemoteEvent::Heartbeat)));
        let Some(Ok(RemoteEvent::Conversation(room))) = stream.next_event() else {
            panic!("expected a room")
        };
        assert_eq!(room.title, "General!");
        assert_eq!(stream.next_event(), None);
        let request = &client.received()[0];
        assert!(request
            .headers
            .iter()
            .any(|h| h.name == "Accept" && h.value == "text/event-stream"));
    }

    #[test]
    fn stream_rejects_bad_json_wrong_room_and_wrong_content_type() {
        let (repo, _) = repo(
            StaticClient::new()
                .route(
                    Method::Get,
                    format!("{BASE}/rooms/general/stream"),
                    sse("event: message\ndata: {\"id\":\"a\",\"conversation_id\":\"other\",\"author\":\"x\",\"body\":\"\",\"sent_at\":1}\n\nevent: message\ndata: nope\n\n"),
                )
                .route(
                    Method::Get,
                    format!("{BASE}/rooms/plain/stream"),
                    json(200, "[]"),
                ),
        );
        let mut stream = repo.open_stream("general", &CancelHandle::new()).unwrap();
        assert!(matches!(
            stream.next_event(),
            Some(Err(ChatError::Protocol { .. }))
        ));
        assert!(matches!(
            stream.next_event(),
            Some(Err(ChatError::Protocol { .. }))
        ));
        assert!(matches!(
            repo.open_stream("plain", &CancelHandle::new())
                .map(|_| ())
                .unwrap_err(),
            ChatError::Protocol { .. }
        ));
        assert!(matches!(
            repo.open_stream("missing", &CancelHandle::new())
                .map(|_| ())
                .unwrap_err(),
            ChatError::NotFound { .. }
        ));
    }
}
