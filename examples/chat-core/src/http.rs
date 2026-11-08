//! [`ChatRepo`] over HTTP + Server-Sent Events, through `forge_net`.
//!
//! The wire protocol (what `examples/chat-server` serves):
//!
//! | Call | Request | Success |
//! |---|---|---|
//! | register | `POST {base}/accounts` `{"display_name"}` | `201` `{"user_id","display_name","token"}` |
//! | who am I | `GET {base}/me` | `200` `{"user_id","display_name"}` |
//! | list | `GET {base}/rooms` | `200` `[Room]` |
//! | create | `POST {base}/rooms` `{"title"}` | `201` `Room` |
//! | mark read | `POST {base}/rooms/{id}/read` `{"up_to"}` | `200` |
//! | load | `GET {base}/rooms/{id}/messages[?since=ms]` | `200` `[Message]`, oldest first, `sent_at > since` |
//! | post | `POST {base}/rooms/{id}/messages` `{"id","author","body"}` | `200`/`201` `Message` (idempotent by `id`) |
//! | stream | `GET {base}/rooms/{id}/stream` | `200` `text/event-stream` |
//!
//! `Room` is `{"id","title","last_message_at","last_message","read_up_to","others_read_up_to"}`
//! (all but the first two optional) and `Message` is
//! `{"id","conversation_id","author","author_id","body","sent_at"}` (times in
//! Unix ms; `author_id` may be missing, from servers or clients without
//! accounts). With an account, every request carries
//! `Authorization: Bearer <token>`; the server then takes the author from
//! the token, and a `401` is [`ChatError::Unauthorized`].
//! The stream sends `event: message` / `event: room` with that JSON as
//! `data`, `event: read` with `{"user_id","up_to"}`, and `: ping` comment lines as heartbeats (the mock server sends
//! one every 15 s). Unknown event names and JSON fields are ignored. A `404`
//! on any room path is [`ChatError::NotFound`].

use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Method, Request, Response};
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
    #[serde(default)]
    last_message: Option<WireMessage>,
    #[serde(default)]
    read_up_to: Option<i64>,
    #[serde(default)]
    others_read_up_to: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WireRead {
    user_id: String,
    up_to: i64,
}

#[derive(Serialize)]
struct WireReadPost {
    up_to: i64,
}

#[derive(Serialize)]
struct WireCreateRoom<'a> {
    title: &'a str,
}

/// Longest conversation title, in characters (the server's limit too).
pub const MAX_TITLE_CHARS: usize = 60;

/// Trims `title` and checks it is usable as a conversation title.
pub fn validate_title(title: &str) -> Result<String, ChatError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(ChatError::invalid("title is empty"));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(ChatError::invalid(format!(
            "title is longer than {MAX_TITLE_CHARS} characters"
        )));
    }
    if title.chars().any(char::is_control) {
        return Err(ChatError::invalid("title contains control characters"));
    }
    Ok(title.to_string())
}

#[derive(Debug, Deserialize)]
struct WireMessage {
    id: String,
    conversation_id: String,
    author: String,
    #[serde(default)]
    author_id: String,
    body: String,
    sent_at: i64,
    #[serde(default)]
    updated_at: Option<i64>,
    #[serde(default)]
    reply_to: Option<String>,
    #[serde(default)]
    edited_at: Option<i64>,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    reactions: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default)]
    attachments: Vec<crate::Attachment>,
}

#[derive(Serialize)]
struct WirePost<'a> {
    id: &'a str,
    author: &'a str,
    body: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    attachments: &'a [crate::Attachment],
}

#[derive(Deserialize)]
struct WireUpload {
    id: String,
}

/// Uploads may take longer than ordinary calls: at least this long.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Serialize)]
struct WireEdit<'a> {
    body: &'a str,
}

#[derive(Serialize)]
struct WireReact<'a> {
    emoji: &'a str,
    on: bool,
}

/// An account on the chat server. The token is a credential: keep it in the
/// platform's secure store (the iOS Keychain), never in plain files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub user_id: String,
    pub display_name: String,
    pub token: String,
}

/// Who the server says a token belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Identity {
    pub user_id: String,
    pub display_name: String,
}

#[derive(Serialize)]
struct WireRegister<'a> {
    display_name: &'a str,
}

/// Longest display name accepted, in characters (the server's limit too).
pub const MAX_DISPLAY_NAME_CHARS: usize = 40;

/// Trims `name` and checks it is usable as a display name.
pub fn validate_display_name(name: &str) -> Result<String, ChatError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ChatError::invalid("display name is empty"));
    }
    if name.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(ChatError::invalid(format!(
            "display name is longer than {MAX_DISPLAY_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ChatError::invalid(
            "display name contains control characters",
        ));
    }
    Ok(name.to_string())
}

impl From<WireRoom> for RemoteConversation {
    fn from(room: WireRoom) -> Self {
        Self {
            id: room.id,
            title: room.title,
            last_message_at: room.last_message_at,
            last_message: room.last_message.map(Into::into),
            read_up_to: room.read_up_to,
            others_read_up_to: room.others_read_up_to,
        }
    }
}

impl From<WireMessage> for Message {
    fn from(m: WireMessage) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            author: m.author,
            author_id: m.author_id,
            body: m.body,
            sent_at: m.sent_at,
            status: MessageStatus::Sent,
            updated_at: m.updated_at,
            reply_to: m.reply_to,
            edited_at: m.edited_at,
            deleted: m.deleted,
            reactions: m.reactions,
            attachments: m.attachments,
        }
    }
}

/// HTTP chat backend. `base_url` is e.g. `http://10.0.2.2:8787` (the Android
/// emulator's view of the host) or `http://127.0.0.1:8787`.
pub struct HttpRepo {
    client: Arc<dyn HttpClient>,
    base_url: String,
    timeout: Duration,
    /// The account token, sent as `Authorization: Bearer`.
    token: Option<String>,
}

impl HttpRepo {
    pub fn new(client: Arc<dyn HttpClient>, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout: DEFAULT_TIMEOUT,
            token: None,
        }
    }

    /// Signs every request with this account token.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Creates an account named `display_name` and returns it with its
    /// token. Needs no token itself.
    pub fn register(&self, display_name: &str) -> Result<Account, ChatError> {
        let display_name = validate_display_name(display_name)?;
        let body = serde_json::to_vec(&WireRegister {
            display_name: &display_name,
        })
        .map_err(ChatError::protocol)?;
        let request = Request::post(format!("{}/accounts", self.base_url), body)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        let response = self.send(request, None)?;
        let account: Account = decode(&response.body)?;
        if account.token.is_empty() || account.user_id.is_empty() {
            return Err(ChatError::protocol(
                "server returned an account without a token",
            ));
        }
        Ok(account)
    }

    /// Who the server says this repo's token belongs to.
    /// [`ChatError::Unauthorized`] if it does not know the token.
    pub fn identity(&self) -> Result<Identity, ChatError> {
        let request =
            Request::get(format!("{}/me", self.base_url)).header("Accept", "application/json");
        let response = self.send(request, None)?;
        decode(&response.body)
    }

    fn authorised(&self, request: Request) -> Request {
        match &self.token {
            Some(token) => request.header("Authorization", format!("Bearer {token}")),
            None => request,
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

    fn message_url(&self, conversation_id: &str, message_id: &str, rest: &str) -> String {
        self.room_url(
            conversation_id,
            &format!("/messages/{}{rest}", encode_path_segment(message_id)),
        )
    }

    /// Sends a change to one message and checks the answer is that message.
    fn changed_message(
        &self,
        request: Request,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Message, ChatError> {
        let response = self.send(
            request.header("Accept", "application/json"),
            Some(conversation_id),
        )?;
        let message: Message = decode::<WireMessage>(&response.body)?.into();
        if message.id != message_id {
            return Err(ChatError::protocol(format!(
                "asked to change {message_id}, the server answered with {}",
                message.id
            )));
        }
        Ok(message)
    }

    fn send(&self, request: Request, room: Option<&str>) -> Result<Response, ChatError> {
        let response = self
            .client
            .send(&self.authorised(request).timeout(self.timeout))?;
        check_status(response.status, &response.body, room)?;
        Ok(response)
    }
}

/// Non-2xx → typed error. A 404 on a room path is `NotFound`.
fn check_status(status: u16, body: &[u8], room: Option<&str>) -> Result<(), ChatError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status == 401 {
        return Err(ChatError::Unauthorized {
            reason: String::from_utf8_lossy(body)
                .trim()
                .chars()
                .take(MAX_REASON_CHARS)
                .collect(),
        });
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
            reply_to: message.reply_to.as_deref(),
            attachments: &message.attachments,
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

    fn upload_attachment(&self, path: &std::path::Path, mime: &str) -> Result<String, ChatError> {
        let request = self
            .authorised(
                Request::post(format!("{}/attachments", self.base_url), Vec::new())
                    .header("Content-Type", mime)
                    .header("Accept", "application/json"),
            )
            .timeout(self.timeout.max(UPLOAD_TIMEOUT));
        let response = self
            .client
            .send_file(&request, path, &forge_net::CancelHandle::new())?;
        check_status(response.status, &response.body, None)?;
        Ok(decode::<WireUpload>(&response.body)?.id)
    }

    fn download_attachment(
        &self,
        id: &str,
        destination: &std::path::Path,
    ) -> Result<(), ChatError> {
        let request = self
            .authorised(Request::get(format!(
                "{}/attachments/{}",
                self.base_url,
                encode_path_segment(id)
            )))
            .timeout(self.timeout);
        let response = forge_net::download_to_file(
            &*self.client,
            &request,
            destination,
            &forge_net::CancelHandle::new(),
            Some(crate::attachments::MAX_ATTACHMENT_BYTES),
        )?;
        check_status(response.status, &response.body, None)
    }

    fn mark_read(&self, conversation_id: &str, up_to: i64) -> Result<(), ChatError> {
        let url = self.room_url(conversation_id, "/read");
        let body = serde_json::to_vec(&WireReadPost { up_to }).map_err(ChatError::protocol)?;
        self.send(
            Request::post(url, body)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json"),
            Some(conversation_id),
        )?;
        Ok(())
    }

    fn create_conversation(&self, title: &str) -> Result<RemoteConversation, ChatError> {
        let title = validate_title(title)?;
        let body =
            serde_json::to_vec(&WireCreateRoom { title: &title }).map_err(ChatError::protocol)?;
        let request = Request::post(format!("{}/rooms", self.base_url), body)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        let response = self.send(request, None)?;
        Ok(decode::<WireRoom>(&response.body)?.into())
    }

    fn edit_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        body: &str,
    ) -> Result<Message, ChatError> {
        let body = serde_json::to_vec(&WireEdit { body }).map_err(ChatError::protocol)?;
        let request = Request::new(
            Method::Patch,
            self.message_url(conversation_id, message_id, ""),
        )
        .body(body)
        .header("Content-Type", "application/json");
        self.changed_message(request, conversation_id, message_id)
    }

    fn delete_message(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Message, ChatError> {
        let request = Request::new(
            Method::Delete,
            self.message_url(conversation_id, message_id, ""),
        );
        self.changed_message(request, conversation_id, message_id)
    }

    fn react(
        &self,
        conversation_id: &str,
        message_id: &str,
        emoji: &str,
        on: bool,
    ) -> Result<Message, ChatError> {
        let body = serde_json::to_vec(&WireReact { emoji, on }).map_err(ChatError::protocol)?;
        let request = Request::post(
            self.message_url(conversation_id, message_id, "/reactions"),
            body,
        )
        .header("Content-Type", "application/json");
        self.changed_message(request, conversation_id, message_id)
    }

    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError> {
        let url = self.room_url(conversation_id, "/stream");
        let request = self
            .authorised(Request::get(url))
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
            "read" => Some(
                decode::<WireRead>(data.as_bytes()).map(|r| RemoteEvent::Read {
                    user_id: r.user_id,
                    up_to: r.up_to,
                }),
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
    fn accounts_register_and_sign_requests() {
        let (repo, client) = repo(
            StaticClient::new()
                .route(
                    Method::Post,
                    format!("{BASE}/accounts"),
                    json(
                        201,
                        r#"{"user_id":"u1","display_name":"Ana","token":"t0k"}"#,
                    ),
                )
                .route(
                    Method::Get,
                    format!("{BASE}/me"),
                    json(401, "unknown token"),
                ),
        );
        let account = repo.register("  Ana ").unwrap();
        assert_eq!(account.user_id, "u1");
        let sent: serde_json::Value = serde_json::from_slice(&client.received()[0].body).unwrap();
        assert_eq!(sent, serde_json::json!({"display_name":"Ana"}));
        assert!(matches!(
            repo.register(" "),
            Err(ChatError::InvalidInput { .. })
        ));

        let signed = HttpRepo::new(client.clone(), BASE).with_token("t0k");
        assert_eq!(
            signed.identity().unwrap_err(),
            ChatError::Unauthorized {
                reason: "unknown token".into()
            }
        );
        let last = client.received().pop().unwrap();
        assert!(last
            .headers
            .iter()
            .any(|h| h.name == "Authorization" && h.value == "Bearer t0k"));
    }

    #[test]
    fn display_names_are_trimmed_and_bounded() {
        assert_eq!(validate_display_name(" Reuben ").unwrap(), "Reuben");
        assert!(validate_display_name(&"x".repeat(MAX_DISPLAY_NAME_CHARS + 1)).is_err());
        assert!(validate_display_name("a\nb").is_err());
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
            author_id: String::new(),
            body: "hi".into(),
            reply_to: None,
            attachments: Vec::new(),
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
