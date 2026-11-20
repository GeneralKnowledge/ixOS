//! Messages between trusted devices on chat-core (Relay M1 stage 4,
//! examples/relay/docs/relay-chat-merge.md step 3).
//!
//! Each trusted device is one conversation, `device:<endpoint id>`, on
//! chat-core's engine, so the Chats screens, the store, search and the
//! outbox are the ones server conversations use. What a server does for a
//! server conversation, this device does for itself:
//!
//! - **The log** ([`PeerLog`]) is the conversation's truth on this device:
//!   every message sent to the device and received from it, rows of
//!   Relay's database (`store`), saved in the same transaction as the
//!   delivery they go with. chat-core's store is a cache of it, as it is of
//!   a server, so a damaged store that chat-core sets aside loses nothing. The log stamps each message as it enters, strictly
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
//!   message `file-<delivery id>` with one attachment, on both sides,
//!   whose id is the BLAKE3 hash of what was sent (a folder's collection,
//!   typed [`FOLDER_MIME`]), the hash iroh-blobs verified it by. The bytes travel by the outbox and iroh-blobs as
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
use forge_net::CancelHandle;
use forge_runtime::lock;
use serde::{Deserialize, Serialize};

use forge_storage::rusqlite::Connection;

use crate::model::RelayError;
use crate::store::{self, Store};
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

/// The attachment of a files delivery whose content has BLAKE3 hash
/// `content` (hex; a folder's is its collection's): the hash is its id,
/// the same on both sides whether or not the file is still here, and
/// nothing needs reading to make it. A folder (several items arrive as
/// one) has type [`FOLDER_MIME`].
pub(crate) fn files_attachment(content: &str, folder: bool, name: &str, size: u64) -> Attachment {
    Attachment {
        id: content.to_string(),
        name: name.to_string(),
        mime: if folder {
            FOLDER_MIME.to_string()
        } else {
            mime_for(name).to_string()
        },
        size,
    }
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
/// The JSON logs' version, before they moved into the database.
pub(crate) const LOG_VERSION: u32 = 1;
/// How often a quiet stream wakes to see if it was cancelled.
const QUIET: Duration = Duration::from_millis(250);
/// How often a quiet stream says it is still there.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// The largest file we send that the conversation keeps a copy of: what
/// chat-core keeps of an attachment.
const KEEP_UP_TO: u64 = chat_core::MAX_ATTACHMENT_BYTES;
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

/// Device conversation `endpoint_id`'s id, if `endpoint_id` can be one
/// (endpoint ids are hex; anything else never names a conversation).
fn checked(endpoint_id: &str) -> Result<String, RelayError> {
    if endpoint_id.is_empty() || !endpoint_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(RelayError::UnknownPeer {
            id: endpoint_id.into(),
        });
    }
    Ok(device_conversation(endpoint_id))
}

/// Changes to device conversations inside one transaction of Relay's
/// database ([`crate::State::change_with`]): they are saved with the
/// record's change, or not at all, and listeners hear of them once the
/// transaction is committed.
pub(crate) struct Conversations<'a> {
    c: &'a Connection,
    changed: Vec<(String, Message)>,
}

impl<'a> Conversations<'a> {
    pub fn new(c: &'a Connection) -> Self {
        Self {
            c,
            changed: Vec::new(),
        }
    }

    /// What changed, by device, for the listeners.
    pub fn changed(self) -> Vec<(String, Message)> {
        self.changed
    }

    pub fn get(&self, endpoint_id: &str, id: &str) -> Option<Message> {
        store::message(self.c, &checked(endpoint_id).ok()?, id)
    }

    /// Our messages to `endpoint_id` still waiting in the outbox.
    pub fn queued(&self, endpoint_id: &str) -> Vec<Message> {
        checked(endpoint_id)
            .map(|conv| store::queued(self.c, &conv))
            .unwrap_or_default()
    }

    /// Adds `message` to `endpoint_id`'s conversation (see [`store::add`]).
    pub fn add(&mut self, endpoint_id: &str, message: Message) -> Result<Message, RelayError> {
        let (message, added) = store::add(self.c, &checked(endpoint_id)?, message)?;
        if added {
            self.changed
                .push((endpoint_id.to_string(), message.clone()));
        }
        Ok(message)
    }

    /// Adds `message` after keeping where its attachment is on this
    /// device, when it is here.
    pub fn add_files(
        &mut self,
        endpoint_id: &str,
        message: Message,
        path: Option<&Path>,
    ) -> Result<Message, RelayError> {
        if let (Some(a), Some(path)) = (message.attachments.first(), path) {
            store::set_file(self.c, &checked(endpoint_id)?, &a.id, path)?;
        }
        self.add(endpoint_id, message)
    }

    /// Moves one of our messages on (see [`store::set_status`]).
    pub fn set_status(
        &mut self,
        endpoint_id: &str,
        id: &str,
        status: MessageStatus,
    ) -> Result<(), RelayError> {
        if let Some(m) = store::set_status(self.c, &checked(endpoint_id)?, id, status)? {
            self.changed.push((endpoint_id.to_string(), m));
        }
        Ok(())
    }

    /// Adds a message moved from elsewhere (see [`store::import`]).
    pub fn import(
        &mut self,
        endpoint_id: &str,
        message: Message,
        path: Option<&Path>,
    ) -> Result<(), RelayError> {
        if let Some(m) = store::import(self.c, &checked(endpoint_id)?, message, path)? {
            self.changed.push((endpoint_id.to_string(), m));
        }
        Ok(())
    }
}

/// One device conversation on this device: its messages are rows of
/// Relay's database, read by query.
pub struct PeerLog {
    conversation_id: String,
    endpoint_id: String,
    /// `devices/<endpoint id>`: copies of files we send ([`Self::keep`]).
    dir: PathBuf,
    logs: Arc<PeerLogsInner>,
}

impl PeerLog {
    fn store(&self) -> &Store {
        &self.logs.store
    }

    pub fn get(&self, id: &str) -> Option<Message> {
        self.store()
            .read(|c| store::message(c, &self.conversation_id, id))
    }

    /// Messages changed after `since` (all without it), oldest first.
    pub fn since(&self, since: Option<i64>) -> Vec<Message> {
        self.store()
            .read(|c| store::since(c, &self.conversation_id, since))
    }

    pub fn last(&self) -> Option<Message> {
        self.store().read(|c| store::last(c, &self.conversation_id))
    }

    /// Our messages still waiting in the outbox.
    pub fn queued(&self) -> Vec<Message> {
        self.store()
            .read(|c| store::queued(c, &self.conversation_id))
    }

    /// Changes this conversation in a transaction of its own.
    fn change<T>(
        &self,
        edit: impl FnOnce(&mut Conversations<'_>) -> Result<T, RelayError>,
    ) -> Result<T, RelayError> {
        let (value, changed) = self.store().write(|c| {
            let mut conversations = Conversations::new(c);
            let value = edit(&mut conversations)?;
            Ok((value, conversations.changed()))
        })?;
        self.logs.notify(changed);
        Ok(value)
    }

    /// Adds `message`, stamped now, unless one with its id is here: then
    /// that one is returned and nothing changes. It is `Sent` unless it is
    /// one of ours `Queued` in the outbox.
    pub fn add(&self, message: Message) -> Result<Message, RelayError> {
        self.change(|c| c.add(&self.endpoint_id, message))
    }

    /// Moves one of our messages on: `Sent` once the device has it,
    /// `Failed` when it can't be delivered, `Queued` when it is sent
    /// again. Its change is stamped, so catch-up after `since` sees it.
    pub fn set_status(&self, id: &str, status: MessageStatus) -> Result<(), RelayError> {
        self.change(|c| c.set_status(&self.endpoint_id, id, status))
    }

    /// A copy of a file we send, kept with the conversation so it still
    /// opens here after the picked file is gone (as chat-core keeps what
    /// it sends). Only up to [`KEEP_UP_TO`]; a larger file is opened where
    /// it was, while it is there.
    pub fn keep(&self, file: &Path, id: &str) -> Option<PathBuf> {
        let size = std::fs::metadata(file).ok()?.len();
        if size > KEEP_UP_TO || !chat_core::is_attachment_id(id) {
            return None;
        }
        let kept = self.dir.join("files").join(id);
        if kept.exists() {
            return Some(kept);
        }
        std::fs::create_dir_all(kept.parent()?).ok()?;
        let partial = kept.with_extension("partial");
        std::fs::copy(file, &partial).ok()?;
        std::fs::rename(&partial, &kept).ok()?;
        Some(kept)
    }

    /// Where attachment `id` is on this device, if it is still there.
    pub fn file(&self, id: &str) -> Option<PathBuf> {
        let path = self
            .store()
            .read(|c| store::file(c, &self.conversation_id, id))?;
        path.exists().then_some(path)
    }

    fn listen(&self) -> mpsc::Receiver<Message> {
        let (tx, rx) = mpsc::channel();
        lock(&self.logs.listeners)
            .entry(self.endpoint_id.clone())
            .or_default()
            .push(tx);
        rx
    }
}

struct PeerLogsInner {
    dir: PathBuf,
    store: Arc<Store>,
    /// Open streams, by device.
    listeners: Mutex<HashMap<String, Vec<mpsc::Sender<Message>>>>,
}

impl PeerLogsInner {
    fn notify(&self, changed: Vec<(String, Message)>) {
        if changed.is_empty() {
            return;
        }
        let mut listeners = lock(&self.listeners);
        for (endpoint_id, message) in changed {
            if let Some(list) = listeners.get_mut(&endpoint_id) {
                list.retain(|l| l.send(message.clone()).is_ok());
            }
        }
    }
}

/// Every device conversation, in Relay's database.
#[derive(Clone)]
pub(crate) struct PeerLogs {
    inner: Arc<PeerLogsInner>,
}

impl PeerLogs {
    /// Conversations in `store`; copies of files we send go in `dir`.
    pub fn new(dir: PathBuf, store: Arc<Store>) -> Self {
        Self {
            inner: Arc::new(PeerLogsInner {
                dir,
                store,
                listeners: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn get(&self, endpoint_id: &str) -> Result<PeerLog, RelayError> {
        Ok(PeerLog {
            conversation_id: checked(endpoint_id)?,
            endpoint_id: endpoint_id.to_string(),
            dir: self.inner.dir.join(endpoint_id),
            logs: Arc::clone(&self.inner),
        })
    }

    /// Tells open streams what a committed transaction changed.
    pub fn notify(&self, changed: Vec<(String, Message)>) {
        self.inner.notify(changed);
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
    log: PeerLog,
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

    /// A device attachment opens from where it is on this device
    /// (`ChatCore::attachment_file` finds it), so a download is asked for
    /// only when it is gone: there is nowhere else to get it from yet.
    fn download_attachment(&self, _id: &str, _destination: &Path) -> Result<(), ChatError> {
        Err(ChatError::InvalidInput {
            reason: "the file is no longer on this device".into(),
        })
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

    /// [`device_file`](Self::device_file) in whichever listed device's
    /// conversation has it.
    pub fn device_attachment(&self, attachment_id: &str) -> Option<String> {
        let listed: Vec<String> = self
            .record
            .get()
            .devices
            .listed()
            .map(|d| d.endpoint_id.clone())
            .collect();
        listed
            .iter()
            .find_map(|id| self.device_file(id, attachment_id))
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

    fn logs(dir: &Path) -> PeerLogs {
        PeerLogs::new(dir.to_path_buf(), Arc::new(Store::in_memory()))
    }

    #[test]
    fn the_log_stamps_in_order_and_adds_each_id_once() {
        let dir = tempfile::tempdir().unwrap();
        let logs = logs(dir.path());
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
        // Another device's stream hears nothing of it.
        assert!(logs.get("../x").is_err());
    }

    #[test]
    fn a_queued_message_moves_on_with_a_new_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let logs = logs(dir.path());
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
