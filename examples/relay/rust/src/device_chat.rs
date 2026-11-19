//! Messages between trusted devices on chat-core (Relay M1 stage 4,
//! examples/relay/docs/relay-chat-merge.md step 3).
//!
//! Each trusted device is one conversation, `device:<endpoint id>`, on
//! chat-core's engine, so the Chats screens, the store, search and the
//! outbox are the ones server conversations use. What a server does for a
//! server conversation, this device does for itself:
//!
//! - **The log** ([`PeerLog`]) is the conversation's truth on this device:
//!   every message sent to the device and received from it, saved in
//!   `devices/<endpoint id>/messages.json`. chat-core's store is a cache of
//!   it, as it is of a server, so a damaged store that chat-core sets aside
//!   loses nothing. The log stamps each message as it enters, strictly
//!   increasing, which is the order both sides show and the `since` cursor
//!   catch-up reads by.
//! - **The repo** ([`PeerRepo`]) is the conversation's `ChatRepo`. Posting a
//!   message adds it to the log as `Queued` and puts it in Relay's outbox
//!   ([`crate::outbox`]), with the files for the same device, so chat-core's
//!   own outbox only hands it over. The outbox sends it over
//!   `relay/device/1` when the device is reached, and the log marks it
//!   `Sent`, or `Failed` (sending it again queues it again).
//! - **Exactly once.** The receiver adds a message to its log, then
//!   answers. A message id it already has is answered without adding it
//!   again, so a retry whose answer was lost never shows twice.
//! - Messages and files to one device keep their order: the outbox sends
//!   oldest first and stops at the first that waits, per device, so a
//!   device that is away holds up only what is for it.
//! - **Files are attachments.** Files sent to a device (Send Files) are a
//!   message `file-<delivery id>` with one attachment, on both sides: a
//!   file's attachment id is the SHA-256 of its bytes, so chat-core checks
//!   what it opens; a folder's is derived from the delivery and its type
//!   is [`FOLDER_MIME`]. The bytes travel by the outbox and iroh-blobs as
//!   before; the log keeps where each attachment is on this device (the
//!   saved file on the receiver, the file that was sent on the sender).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use chat_core::{
    Attachment, ChatError, ChatRepo, Message, MessageStatus, OutgoingMessage, RemoteConversation,
    RemoteEvent, RemoteStream,
};
use forge_app::Document;
use forge_net::CancelHandle;
use forge_runtime::{lock, now_ms};
use serde::{Deserialize, Serialize};

use crate::model::RelayError;
use crate::RelayCore;

/// The conversation id of the trusted device `endpoint_id`.
pub fn device_conversation(endpoint_id: &str) -> String {
    format!("{PREFIX}{endpoint_id}")
}

/// The trusted device a conversation id names, if it names one.
pub fn conversation_device(conversation_id: &str) -> Option<&str> {
    conversation_id.strip_prefix(PREFIX)
}

const PREFIX: &str = "device:";
/// The type of a folder sent as one attachment.
pub const FOLDER_MIME: &str = "inode/directory";

/// The message id of the files in delivery `delivery` (hex), on both sides.
pub(crate) fn files_message_id(delivery: &str) -> String {
    format!("file-{delivery}")
}

/// The attachment of what delivery `delivery` (hex) carries: `file` is
/// the one file it is, kept on this device; anything else (a folder, or
/// several items, which arrive as a folder) is a folder. A file's id is
/// its SHA-256, a folder's the SHA-256 of the delivery's id, the same on
/// both sides.
pub(crate) fn files_attachment(
    delivery: &str,
    file: Option<&Path>,
    name: &str,
    size: u64,
) -> Result<Attachment, RelayError> {
    let (id, mime) = if let Some(path) = file.filter(|p| p.is_file()) {
        let id = forge_storage::files::sha256_file(path).map_err(|e| RelayError::File {
            reason: e.to_string(),
        })?;
        (id, mime_for(name).to_string())
    } else {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(format!("relay-folder:{delivery}").as_bytes());
        (crate::hex::hex(&digest), FOLDER_MIME.to_string())
    };
    Ok(Attachment {
        id,
        name: name.to_string(),
        mime,
        size,
    })
}

/// A file's type from its name, for how Chats shows it.
fn mime_for(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}
const LOG_VERSION: u32 = 1;
/// How often a quiet stream wakes to see if it was cancelled.
const QUIET: Duration = Duration::from_millis(250);
/// How often a quiet stream says it is still there.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// Longest message id accepted from another device.
pub(crate) const MAX_ID_CHARS: usize = 100;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LogFile {
    #[serde(default)]
    pub messages: Vec<Message>,
    /// The newest stamp given, so stamps only ever increase.
    #[serde(default)]
    pub last_at: i64,
    /// Where each attachment is on this device, by attachment id.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

/// One device conversation's messages on this device.
pub struct PeerLog {
    conversation_id: String,
    doc: Mutex<Document<LogFile>>,
    listeners: Mutex<Vec<mpsc::Sender<Message>>>,
}

impl PeerLog {
    fn open(dir: PathBuf, endpoint_id: &str) -> Result<Self, RelayError> {
        std::fs::create_dir_all(&dir).map_err(crate::node::file_error)?;
        Ok(Self {
            conversation_id: device_conversation(endpoint_id),
            doc: Mutex::new(Document::open(dir.join("messages.json"), LOG_VERSION)?),
            listeners: Mutex::new(Vec::new()),
        })
    }

    pub fn get(&self, id: &str) -> Option<Message> {
        lock(&self.doc)
            .value()
            .messages
            .iter()
            .find(|m| m.id == id)
            .cloned()
    }

    /// Messages changed after `since` (all without it), oldest first.
    pub fn since(&self, since: Option<i64>) -> Vec<Message> {
        lock(&self.doc)
            .value()
            .messages
            .iter()
            .filter(|m| since.is_none_or(|s| m.changed_at() > s))
            .cloned()
            .collect()
    }

    pub fn last(&self) -> Option<Message> {
        lock(&self.doc).value().messages.last().cloned()
    }

    /// Adds `message`, stamped now, unless one with its id is here: then
    /// that one is returned and nothing changes. It is `Sent` unless it is
    /// one of ours `Queued` in the outbox.
    pub fn add(&self, mut message: Message) -> Result<Message, RelayError> {
        let added = {
            let mut doc = lock(&self.doc);
            if let Some(found) = doc.value().messages.iter().find(|m| m.id == message.id) {
                return Ok(found.clone());
            }
            message.conversation_id = self.conversation_id.clone();
            if message.status != MessageStatus::Queued {
                message.status = MessageStatus::Sent;
            }
            message.updated_at = None;
            doc.change(|log| {
                let at = now_ms().max(log.last_at + 1);
                log.last_at = at;
                message.sent_at = at;
                log.messages.push(message.clone());
                Ok::<_, RelayError>(())
            })?;
            message
        };
        lock(&self.listeners).retain(|l| l.send(added.clone()).is_ok());
        Ok(added)
    }

    /// Moves one of our messages on: `Sent` once the device has it,
    /// `Failed` when it can't be delivered, `Queued` when it is sent
    /// again. Its change is stamped, so catch-up after `since` sees it.
    pub fn set_status(&self, id: &str, status: MessageStatus) -> Result<(), RelayError> {
        let changed = {
            let mut doc = lock(&self.doc);
            let Some(i) = doc.value().messages.iter().position(|m| m.id == id) else {
                return Ok(());
            };
            if doc.value().messages[i].status == status {
                return Ok(());
            }
            doc.change(|log| {
                let at = now_ms().max(log.last_at + 1);
                log.last_at = at;
                let m = &mut log.messages[i];
                m.status = status;
                m.updated_at = Some(at);
                Ok::<_, RelayError>(m.clone())
            })?
        };
        lock(&self.listeners).retain(|l| l.send(changed.clone()).is_ok());
        Ok(())
    }

    /// Adds `message` (as [`add`](Self::add)) after keeping where its
    /// attachment is on this device, when it is here.
    pub fn add_files(&self, message: Message, path: Option<&Path>) -> Result<Message, RelayError> {
        if let (Some(a), Some(path)) = (message.attachments.first(), path) {
            let (id, path) = (a.id.clone(), path.to_string_lossy().into_owned());
            lock(&self.doc).change(|log| {
                log.files.insert(id, path);
                Ok::<_, RelayError>(())
            })?;
        }
        self.add(message)
    }

    /// Where attachment `id` is on this device, if it is still there.
    pub fn file(&self, id: &str) -> Option<PathBuf> {
        let path = PathBuf::from(lock(&self.doc).value().files.get(id)?);
        path.exists().then_some(path)
    }

    /// Our messages still waiting in the outbox.
    pub fn queued(&self) -> Vec<Message> {
        lock(&self.doc)
            .value()
            .messages
            .iter()
            .filter(|m| m.status == MessageStatus::Queued)
            .cloned()
            .collect()
    }

    fn listen(&self) -> mpsc::Receiver<Message> {
        let (tx, rx) = mpsc::channel();
        lock(&self.listeners).push(tx);
        rx
    }
}

/// The logs of every device conversation, opened on first use.
pub(crate) struct PeerLogs {
    dir: PathBuf,
    open: Mutex<HashMap<String, Arc<PeerLog>>>,
}

impl PeerLogs {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            open: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, endpoint_id: &str) -> Result<Arc<PeerLog>, RelayError> {
        let mut open = lock(&self.open);
        if let Some(log) = open.get(endpoint_id) {
            return Ok(Arc::clone(log));
        }
        // Endpoint ids are hex; anything else never names a folder.
        if endpoint_id.is_empty() || !endpoint_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(RelayError::UnknownPeer {
                id: endpoint_id.into(),
            });
        }
        let log = Arc::new(PeerLog::open(self.dir.join(endpoint_id), endpoint_id)?);
        open.insert(endpoint_id.to_string(), Arc::clone(&log));
        Ok(log)
    }
}

/// A message as it crosses `relay/device/1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PeerMessage {
    pub id: String,
    pub body: String,
    pub reply_to: Option<String>,
}

/// The `ChatRepo` of one trusted device's conversation.
pub struct PeerRepo {
    relay: Weak<RelayCore>,
    endpoint_id: String,
    log: Arc<PeerLog>,
}

impl PeerRepo {
    pub(crate) fn new(relay: &Arc<RelayCore>, endpoint_id: &str) -> Result<Self, RelayError> {
        Ok(Self {
            relay: Arc::downgrade(relay),
            endpoint_id: endpoint_id.to_string(),
            log: relay.peer_logs.get(endpoint_id)?,
        })
    }

    fn relay(&self) -> Result<Arc<RelayCore>, ChatError> {
        self.relay.upgrade().ok_or(ChatError::ShutDown)
    }

    fn check(&self, conversation_id: &str) -> Result<(), ChatError> {
        if conversation_id == self.log.conversation_id {
            Ok(())
        } else {
            Err(ChatError::NotFound {
                conversation_id: conversation_id.into(),
            })
        }
    }
}

/// What a failed delivery means to chat-core: a device that wasn't
/// reached is a network error (the message waits in the outbox); one that
/// refused it is not (the message fails, and the person can retry).
fn chat_error(err: RelayError) -> ChatError {
    match err {
        RelayError::Unreachable { reason } | RelayError::Connection { reason } => {
            ChatError::Network { reason }
        }
        RelayError::Stopped { .. } => ChatError::Network {
            reason: "Relay stopped".into(),
        },
        RelayError::Refused => ChatError::Http {
            status: 403,
            reason: "the other device doesn't trust this one".into(),
        },
        RelayError::Declined { reason } => ChatError::Http {
            status: 403,
            reason,
        },
        RelayError::UnknownPeer { .. } => ChatError::Http {
            status: 403,
            reason: "no longer trusted".into(),
        },
        other => ChatError::Protocol {
            reason: other.to_string(),
        },
    }
}

impl ChatRepo for PeerRepo {
    fn list_conversations(&self) -> Result<Vec<RemoteConversation>, ChatError> {
        let relay = self.relay()?;
        let title = relay
            .record
            .get()
            .devices
            .get(&self.endpoint_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Device".into());
        let last = self.log.last();
        Ok(vec![RemoteConversation {
            id: self.log.conversation_id.clone(),
            title,
            last_message_at: last.as_ref().map(|m| m.sent_at),
            last_message: last,
            read_up_to: None,
            others_read_up_to: None,
        }])
    }

    fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError> {
        self.check(conversation_id)?;
        Ok(self.log.since(since))
    }

    fn post_message(&self, message: &OutgoingMessage) -> Result<Message, ChatError> {
        self.check(&message.conversation_id)?;
        if !message.attachments.is_empty() {
            return Err(ChatError::UnsupportedOnHost {
                detail: "files go to a device from its Send Files".into(),
            });
        }
        // Handed to Relay's outbox, which delivers it: chat-core's own
        // outbox doesn't send it again.
        let relay = self.relay()?;
        let queued = forge_runtime::block_on(relay.queue_message(
            &self.endpoint_id,
            Message {
                id: message.id.clone(),
                author: message.author.clone(),
                author_id: message.author_id.clone(),
                body: message.body.clone(),
                reply_to: message.reply_to.clone(),
                ..Message::default()
            },
        ))
        .map_err(chat_error)?;
        Ok(queued.0)
    }

    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError> {
        self.check(conversation_id)?;
        if cancel.is_cancelled() {
            return Err(ChatError::Cancelled);
        }
        Ok(Box::new(LogStream {
            rx: self.log.listen(),
            cancel: cancel.clone(),
            quiet_for: Duration::ZERO,
        }))
    }

    /// Copies the attachment from where it is on this device; chat-core
    /// checks the bytes against the id. A folder isn't one file: it opens
    /// from its place in Files.
    fn download_attachment(&self, id: &str, destination: &Path) -> Result<(), ChatError> {
        let invalid = |reason: &str| ChatError::InvalidInput {
            reason: reason.into(),
        };
        let path = self
            .log
            .file(id)
            .ok_or_else(|| invalid("the file is no longer on this device"))?;
        if path.is_dir() {
            return Err(invalid("a folder opens from Files, not as one file"));
        }
        std::fs::copy(&path, destination).map_err(|err| ChatError::UnsupportedOnHost {
            detail: format!("cannot copy {}: {err}", path.display()),
        })?;
        Ok(())
    }
}

/// The `relay` command's and the app's side of device files.
impl RelayCore {
    /// Where attachment `attachment_id` of `endpoint_id`'s conversation is
    /// on this device (a file or a folder), if it is still there.
    pub fn device_file(&self, endpoint_id: &str, attachment_id: &str) -> Option<String> {
        let log = self.peer_logs.get(endpoint_id).ok()?;
        Some(log.file(attachment_id)?.to_string_lossy().into_owned())
    }
}

struct LogStream {
    rx: mpsc::Receiver<Message>,
    cancel: CancelHandle,
    quiet_for: Duration,
}

impl RemoteStream for LogStream {
    fn next_event(&mut self) -> Option<Result<RemoteEvent, ChatError>> {
        loop {
            if self.cancel.is_cancelled() {
                return Some(Err(ChatError::Cancelled));
            }
            match self.rx.recv_timeout(QUIET) {
                Ok(message) => {
                    self.quiet_for = Duration::ZERO;
                    return Some(Ok(RemoteEvent::Message(message)));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.quiet_for += QUIET;
                    if self.quiet_for >= HEARTBEAT {
                        self.quiet_for = Duration::ZERO;
                        return Some(Ok(RemoteEvent::Heartbeat));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }
}

/// The `relay` command's side of device conversations. The app's
/// messages reach the same outbox through chat-core (`ChatCore::use_relay`).
impl RelayCore {
    /// The conversation with a trusted device, oldest first.
    pub fn device_messages(&self, endpoint_id: &str) -> Result<Vec<Message>, RelayError> {
        Ok(self.peer_logs.get(endpoint_id)?.since(None))
    }

    /// Sends `text` to a trusted device's conversation, through the outbox,
    /// and waits for the first attempt: the message comes back `Sent`, or
    /// `Queued` when the device wasn't reached (it goes when it is). A
    /// refusal is an error.
    pub async fn send_device_message(
        &self,
        endpoint_id: &str,
        text: String,
    ) -> Result<Message, RelayError> {
        let body = crate::model::clean_note(&text)?;
        let me = self.identity();
        let message = Message {
            id: crate::hex::hex(&crate::outbox::new_id()),
            author: me.name,
            author_id: format!("relay:{}", me.device_id),
            body,
            ..Message::default()
        };
        let (queued, first) = self.queue_message(endpoint_id, message).await?;
        first.await.unwrap_or(Ok(()))?;
        Ok(self
            .peer_logs
            .get(endpoint_id)?
            .get(&queued.id)
            .unwrap_or(queued))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_stamps_in_order_and_adds_each_id_once() {
        let dir = tempfile::tempdir().unwrap();
        let logs = PeerLogs::new(dir.path().to_path_buf());
        let log = logs.get("abc123").unwrap();
        let rx = log.listen();
        let message = |id: &str| Message {
            id: id.into(),
            body: id.into(),
            ..Message::default()
        };
        let a = log.add(message("a")).unwrap();
        let b = log.add(message("b")).unwrap();
        assert!(b.sent_at > a.sent_at);
        assert_eq!(a.conversation_id, "device:abc123");
        assert_eq!(a.status, MessageStatus::Sent);
        let again = log.add(message("a")).unwrap();
        assert_eq!(again, a);
        assert_eq!(log.since(None).len(), 2);
        assert_eq!(log.since(Some(a.sent_at)), vec![b.clone()]);
        assert_eq!(rx.try_iter().count(), 2, "one event per new message");

        // It is all there after a relaunch.
        drop(logs);
        let logs = PeerLogs::new(dir.path().to_path_buf());
        assert_eq!(logs.get("abc123").unwrap().since(None), vec![a, b]);
        assert!(logs.get("../x").is_err());
    }

    #[test]
    fn a_queued_message_moves_on_with_a_new_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let logs = PeerLogs::new(dir.path().to_path_buf());
        let log = logs.get("abc123").unwrap();
        let queued = log
            .add(Message {
                id: "q".into(),
                body: "hi".into(),
                status: MessageStatus::Queued,
                ..Message::default()
            })
            .unwrap();
        assert_eq!(queued.status, MessageStatus::Queued);
        assert_eq!(log.queued().len(), 1);
        let rx = log.listen();
        log.set_status("q", MessageStatus::Sent).unwrap();
        let sent = log.get("q").unwrap();
        assert_eq!(sent.status, MessageStatus::Sent);
        assert!(
            sent.changed_at() > queued.sent_at,
            "catch-up sees the change"
        );
        assert_eq!(log.since(Some(queued.sent_at)), vec![sent.clone()]);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![sent]);
        assert!(log.queued().is_empty());
        log.set_status("missing", MessageStatus::Sent).unwrap();
    }

    #[test]
    fn only_a_device_that_wasnt_reached_keeps_a_message_waiting() {
        let unreachable = chat_error(RelayError::Unreachable { reason: "x".into() });
        assert!(unreachable.is_transient());
        assert!(!chat_error(RelayError::Refused).is_transient());
        assert!(!chat_error(RelayError::Declined { reason: "x".into() }).is_transient());
        assert_eq!(conversation_device(&device_conversation("ab")), Some("ab"));
        assert_eq!(conversation_device("room-1"), None);
    }
}
