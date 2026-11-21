//! `relay/device/1`: notes and files between trusted devices on iroh, with
//! no code (brief PR 3, examples/relay/docs/relay-m1-m2-brief.md, "One
//! transfer path", step 3).
//!
//! - The connection is authenticated by the endpoint ids: the listener
//!   accepts it only from a device it trusts ([`crate::trust`]), and closes
//!   it at once otherwise, before reading anything.
//! - One item per connection. The dialer says [`DeviceMsg::Hello`], the
//!   listener answers with its own, then the dialer offers a note or a
//!   file, and the listener answers [`DeviceMsg::Done`] or
//!   [`DeviceMsg::Refused`].
//! - A file (or folder, as a collection) is offered by its root hash, and
//!   the listener fetches it with iroh-blobs from the dialer, which allows
//!   exactly those blobs to exactly that endpoint while the offer stands.
//!   The listener ticks with how much it has while it fetches.
//! - Messages are CBOR. Their bytes are pinned by golden fixtures in the
//!   tests below, so two builds of Relay keep understanding each other.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{EndpointAddr, EndpointId};
use iroh_blobs::{Hash, HashAndFormat};
use serde::{Deserialize, Serialize};

use forge_services::ai::{AiError, ChatRequest, LanguageModel, TextOut};

use crate::code::{connection, get_within, put, CodeProgress, STEP};
use crate::model::{RelayError, MAX_FILE_BYTES};
use crate::node::{lock, Node};
use crate::resolver::{route_of, ConnectionMode, RouteKind};

pub(crate) const DEVICE_ALPN: &[u8] = b"forge-relay/device/1";
/// The protocol's version inside `relay/device/1`.
pub(crate) const VERSION: u16 = 1;
/// How often the receiver says how far it has got.
const TICK: Duration = Duration::from_secs(2);
/// How often a file on its way is checked against trust (revoked: stop).
const TRUST_CHECK: Duration = if cfg!(test) {
    Duration::from_millis(20)
} else {
    Duration::from_millis(250)
};
/// How long the sender waits without a tick before giving up.
const SILENCE: Duration = Duration::from_secs(30);
/// The close code for a device that isn't trusted.
const NOT_TRUSTED: u32 = 403;
/// How long an asker waits for the next piece of an answer: a big model on
/// a laptop's CPU can take minutes to read a long prompt.
pub(crate) const ANSWER_GAP: Duration = Duration::from_secs(300);
/// How often a waiting asker checks whether it should say stop.
const STOP_CHECK: Duration = Duration::from_millis(100);

/// Everything said on `relay/device/1`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) enum DeviceMsg {
    /// First from each side: the protocol version, the device's name and
    /// where it can be reached ("host:port", for the resolver; added in M2
    /// PR 6, left out when empty so older devices read it unchanged, and
    /// ignored by them when present).
    Hello {
        version: u16,
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        addresses: Vec<String>,
    },
    Note {
        /// Chosen by the sender, kept through retries (the outbox's id).
        id: [u8; 16],
        text: String,
    },
    File {
        id: [u8; 16],
        /// The root blob: one file, or a folder's collection.
        hash: [u8; 32],
        collection: bool,
        name: String,
        size: u64,
    },
    /// The receiver has this many bytes of the file so far.
    Tick { done: u64 },
    /// The receiver has the item, verified and saved.
    Done,
    /// The receiver won't take it, and why, in words for the sender.
    Refused { reason: String },
    /// A message in the two devices' conversation (`device_chat`), by the
    /// id chat-core gave it, kept through retries.
    Message {
        id: String,
        body: String,
        reply_to: Option<String>,
    },
    /// Instead of an item: "tell me about yourself" (brief PR 5). Any
    /// trusted device may ask; the answer is `Status`.
    StatusAsk,
    /// What a device says about itself: read-only, its own claim. `offers`
    /// names what it does over `relay/device/1`, such as "notes", "files",
    /// "messages" or "ai".
    Status {
        name: String,
        platform: String,
        offers: Vec<String>,
    },
    /// Instead of an item: asks this device's language model (Forge AI's
    /// request, OpenAI chat-completions shaped). Answered with `Delta`s,
    /// then `Answered` or `Failed`; or `Refused` when it answers no asks.
    /// Moved here from the old TCP protocol unchanged (M1 PR 3b).
    Ask { request: ChatRequest },
    /// More of the answer.
    Delta { text: String },
    /// The answer is complete; `model` is the model that wrote it.
    Answered { model: String },
    /// The model gave no answer.
    Failed { error: AiError },
    /// The asker wants no more of the answer: stop the model.
    Stop,
    /// Instead of an item: "which devices do you trust?" (My Devices,
    /// step 1). Answered with `DeviceList` only to a device the person let
    /// see them; `Refused` otherwise.
    DevicesAsk,
    /// The devices this one trusts (endpoint id, name) and has revoked,
    /// without the asker.
    DeviceList {
        trusted: Vec<(String, String)>,
        revoked: Vec<String>,
    },
}

impl DeviceMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        ciborium::into_writer(self, &mut out).expect("CBOR into a Vec");
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RelayError> {
        ciborium::from_reader(bytes).map_err(|e| connection(format!("bad message: {e}")))
    }
}

async fn say(send: &mut SendStream, msg: &DeviceMsg) -> Result<(), RelayError> {
    put(send, &msg.encode()).await
}

async fn hear(recv: &mut RecvStream, within: Duration) -> Result<DeviceMsg, RelayError> {
    DeviceMsg::decode(&get_within(recv, within).await?)
}

/// A name from another device made safe to show and to save under: its
/// last path part, without control or direction-changing characters, at
/// most 255 bytes. `fallback` when nothing is left.
pub(crate) fn clean_offered_name(name: &str, fallback: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut clean: String = last
        .chars()
        .filter(|c| {
            !c.is_control()
                // Bidi embeddings, overrides and isolates, and marks.
                && !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    clean = clean.trim().to_string();
    while clean.len() > 255 {
        clean.pop();
    }
    if clean.is_empty() || clean == "." || clean == ".." {
        fallback.to_string()
    } else {
        clean
    }
}

/// What the listening side asks of Relay's core.
pub(crate) trait DeviceInbound: Send + Sync + 'static {
    /// The device's rights here, or `None` when it isn't trusted: then the
    /// connection is closed before anything is read.
    fn rights(&self, from: &str) -> Option<Rights>;
    /// Whether `from` is still trusted and may send here, asked again
    /// before something it sent is saved: it may have been revoked while
    /// the transfer ran.
    fn still_takes_from(&self, from: &str) -> bool {
        self.rights(from).is_some_and(|r| r.receives_from)
    }
    /// This device's name, for the reply to Hello.
    fn my_name(&self) -> String;
    /// What this device says about itself, for a `StatusAsk`.
    fn status(&self) -> StatusReport;
    /// The devices trusted and revoked here, for `asker` (`DevicesAsk`).
    fn device_list(&self, _asker: &str) -> crate::trust::DeviceList {
        crate::trust::DeviceList::default()
    }
    /// A trusted connection from `from` got past Hello: it was reached.
    fn reached(&self, from: &str);
    /// A trusted device said where it can be reached, and connected over
    /// `route`.
    fn met(&self, _from: &str, _addresses: Vec<String>, _route: Option<RouteKind>) {}
    /// The model this device answers asks with, if it answers any.
    fn model(&self) -> Option<Arc<dyn LanguageModel>> {
        None
    }
    /// An ask from `from` ended (for the log).
    fn answered(&self, _from: &str, _report: &AskReport) {}
    /// Where received files go.
    fn folder(&self) -> PathBuf;
    /// Whether the item `id` from `from` is here already: a retry whose
    /// answer was lost, answered Done without saving it again.
    fn received_before(&self, from: &str, id: &[u8; 16]) -> bool;
    /// A note arrived; returns when it is saved, with its id remembered.
    fn note(&self, from: &str, id: [u8; 16], text: String) -> Result<(), RelayError>;
    /// A conversation message arrived; returns when it is in the log (a
    /// message whose id is there already is not added again).
    fn message(
        &self,
        from: &str,
        message: crate::device_chat::PeerMessage,
    ) -> Result<(), RelayError>;
    /// A file is coming; returns the history entry's id.
    fn begin_file(&self, from: &str, name: &str, size: u64) -> Result<u64, RelayError>;
    fn progress(&self, id: u64, done: u64);
    /// It ended: saved at the path (and the item's id `delivery` from
    /// `from` remembered), or failed with the reason. `content` is the
    /// BLAKE3 hash of what was sent (a folder's collection).
    fn finish_file(
        &self,
        id: u64,
        from: &str,
        delivery: [u8; 16],
        content: [u8; 32],
        result: Result<String, String>,
    );
}

/// Trusts everybody and takes nothing: for tests of the protocol itself.
#[cfg(test)]
pub(crate) struct TrustsAll;

#[cfg(test)]
impl DeviceInbound for TrustsAll {
    fn rights(&self, _: &str) -> Option<Rights> {
        Some(Rights {
            receives_from: true,
            may_ask: true,
            sees_devices: true,
        })
    }
    fn my_name(&self) -> String {
        "listener".into()
    }
    fn status(&self) -> StatusReport {
        StatusReport::default()
    }
    fn reached(&self, _: &str) {}
    fn folder(&self) -> PathBuf {
        PathBuf::from("/nonexistent")
    }
    fn received_before(&self, _: &str, _: &[u8; 16]) -> bool {
        false
    }
    fn note(&self, _: &str, _: [u8; 16], _: String) -> Result<(), RelayError> {
        Ok(())
    }
    fn message(&self, _: &str, _: crate::device_chat::PeerMessage) -> Result<(), RelayError> {
        Ok(())
    }
    fn begin_file(&self, _: &str, _: &str, _: u64) -> Result<u64, RelayError> {
        Err(RelayError::Refused)
    }
    fn progress(&self, _: u64, _: u64) {}
    fn finish_file(&self, _: u64, _: &str, _: [u8; 16], _: [u8; 32], _: Result<String, String>) {}
}

/// Trusts nobody: for nodes in tests that only use word codes.
#[cfg(test)]
pub(crate) struct NoDevices;

#[cfg(test)]
impl DeviceInbound for NoDevices {
    fn rights(&self, _: &str) -> Option<Rights> {
        None
    }
    fn my_name(&self) -> String {
        "test".into()
    }
    fn status(&self) -> StatusReport {
        StatusReport::default()
    }
    fn reached(&self, _: &str) {}
    fn folder(&self) -> PathBuf {
        PathBuf::from("/nonexistent")
    }
    fn received_before(&self, _: &str, _: &[u8; 16]) -> bool {
        false
    }
    fn note(&self, _: &str, _: [u8; 16], _: String) -> Result<(), RelayError> {
        Err(RelayError::Refused)
    }
    fn message(&self, _: &str, _: crate::device_chat::PeerMessage) -> Result<(), RelayError> {
        Err(RelayError::Refused)
    }
    fn begin_file(&self, _: &str, _: &str, _: u64) -> Result<u64, RelayError> {
        Err(RelayError::Refused)
    }
    fn progress(&self, _: u64, _: u64) {}
    fn finish_file(&self, _: u64, _: &str, _: [u8; 16], _: [u8; 32], _: Result<String, String>) {}
}

/// What a device says about itself in a `Status`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatusReport {
    pub name: String,
    pub platform: String,
    pub offers: Vec<String>,
}

/// How an answered ask went, for the answering side's log.
#[derive(Clone, Debug)]
pub struct AskReport {
    /// The model that answered.
    pub model: String,
    /// How many pieces of text it wrote.
    pub pieces: u64,
    pub elapsed: Duration,
    /// Why it gave no answer, if it didn't.
    pub error: Option<String>,
}

/// What a trusted device may do here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rights {
    pub receives_from: bool,
    /// May ask this device's AI: its own switch, off by default, never
    /// implied by trust.
    pub may_ask: bool,
    /// May see the devices trusted here (My Devices): its own switch, off
    /// by default.
    pub sees_devices: bool,
}

/// The node, for the protocol handler, which is made before the node is.
pub(crate) type NodeLink = Arc<OnceLock<Weak<Node>>>;

/// The listening side of `relay/device/1`.
#[derive(Clone)]
pub(crate) struct DeviceHandler {
    pub host: Arc<dyn DeviceInbound>,
    pub node: NodeLink,
}

impl std::fmt::Debug for DeviceHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceHandler")
    }
}

impl ProtocolHandler for DeviceHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let them = conn.remote_id().to_string();
        let Some(rights) = self.host.rights(&them) else {
            log::info!("device: refused a device that isn't trusted");
            conn.close(NOT_TRUSTED.into(), b"not trusted");
            return Ok(());
        };
        let Some(node) = self.node.get().and_then(Weak::upgrade) else {
            conn.close(0u32.into(), b"closing");
            return Ok(());
        };
        if let Err(err) = self.answer(&conn, &them, rights, &node).await {
            log::info!("device: a connection ended: {err}");
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), conn.closed()).await;
        conn.close(0u32.into(), b"bye");
        Ok(())
    }
}

impl DeviceHandler {
    /// Runs one ask: the model writes on a blocking thread while its text
    /// goes out as it comes, and a listener waits for the asker's `Stop`.
    /// If the asker hangs up, the model is stopped too.
    async fn answer_ask(
        &self,
        mut send: SendStream,
        mut recv: RecvStream,
        them: &str,
        request: ChatRequest,
    ) -> Result<(), RelayError> {
        let host = Arc::clone(&self.host);
        let Some(model) = tokio::task::spawn_blocking(move || host.model())
            .await
            .map_err(connection)?
        else {
            let reason = "The other device doesn't answer asks".into();
            say(&mut send, &DeviceMsg::Refused { reason }).await?;
            send.finish().ok();
            return Ok(());
        };
        let started = std::time::Instant::now();
        let name = model.describe().id;
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let pipe = Arc::new(Pipe {
            tx,
            stop: Arc::clone(&stop),
        });
        let mut worker = tokio::task::spawn_blocking(move || model.generate(request, pipe));
        // The asker's only word now is Stop; anything else, or hanging up,
        // stops the model too.
        let listener = tokio::spawn({
            let stop = Arc::clone(&stop);
            async move {
                let _ = hear(&mut recv, Duration::from_secs(24 * 3600)).await;
                stop.store(true, Ordering::SeqCst);
            }
        });
        let mut pieces = 0u64;
        let mut gone = false;
        let result = loop {
            tokio::select! {
                biased;
                Some(text) = rx.recv() => {
                    pieces += 1;
                    if !gone && say(&mut send, &DeviceMsg::Delta { text }).await.is_err() {
                        stop.store(true, Ordering::SeqCst);
                        gone = true;
                    }
                }
                done = &mut worker => break done,
            }
        };
        listener.abort();
        while let Ok(text) = rx.try_recv() {
            pieces += 1;
            if !gone && say(&mut send, &DeviceMsg::Delta { text }).await.is_err() {
                gone = true;
            }
        }
        let result = result.unwrap_or_else(|_| {
            Err(AiError::Engine {
                reason: "the model crashed".into(),
            })
        });
        let result = match result {
            Ok(()) if stop.load(Ordering::SeqCst) => Err(AiError::Stopped),
            other => other,
        };
        self.host.answered(
            them,
            &AskReport {
                model: name.clone(),
                pieces,
                elapsed: started.elapsed(),
                error: result.as_ref().err().map(|e| e.to_string()),
            },
        );
        if gone {
            return Err(connection("the asker went away"));
        }
        let last = match result {
            Ok(()) => DeviceMsg::Answered { model: name },
            Err(error) => DeviceMsg::Failed { error },
        };
        say(&mut send, &last).await?;
        send.finish().ok();
        Ok(())
    }

    async fn answer(
        &self,
        conn: &Connection,
        them: &str,
        rights: Rights,
        node: &Node,
    ) -> Result<(), RelayError> {
        let (mut send, mut recv) = tokio::time::timeout(STEP, conn.accept_bi())
            .await
            .map_err(|_| connection("no stream"))?
            .map_err(connection)?;
        match hear(&mut recv, STEP).await? {
            DeviceMsg::Hello {
                version, addresses, ..
            } if version == VERSION => {
                self.host.met(them, addresses, route_of(conn));
            }
            DeviceMsg::Hello { .. } => {
                let reason = "The other device's Relay is a different version; update both".into();
                say(&mut send, &DeviceMsg::Refused { reason }).await?;
                return Ok(());
            }
            _ => return Err(connection("no hello")),
        }
        let host = Arc::clone(&self.host);
        let name = tokio::task::spawn_blocking(move || host.my_name())
            .await
            .map_err(connection)?;
        say(
            &mut send,
            &DeviceMsg::Hello {
                version: VERSION,
                name,
                addresses: node.addresses(),
            },
        )
        .await?;
        self.host.reached(them);
        // An item this build can't read (a newer build's) is refused with
        // a reason, so the sender ends it instead of retrying it for days.
        let item = match DeviceMsg::decode(&get_within(&mut recv, STEP).await?) {
            Ok(item) => item,
            Err(_) => {
                say(&mut send, &unsupported()).await?;
                return Ok(());
            }
        };
        let item = match item {
            // Asks need their own switch: trust alone doesn't grant them.
            DeviceMsg::Ask { .. } if !rights.may_ask => {
                say(
                    &mut send,
                    &DeviceMsg::Refused {
                        reason: NOT_ASKED.into(),
                    },
                )
                .await?;
                send.finish().ok();
                return Ok(());
            }
            DeviceMsg::Ask { request } => return self.answer_ask(send, recv, them, request).await,
            other => other,
        };
        // The device list has its own switch: trust alone doesn't share it.
        if item == DeviceMsg::DevicesAsk {
            let reply = if rights.sees_devices {
                let host = Arc::clone(&self.host);
                let asker = them.to_string();
                let list = tokio::task::spawn_blocking(move || host.device_list(&asker))
                    .await
                    .map_err(connection)?;
                DeviceMsg::DeviceList {
                    trusted: list.trusted,
                    revoked: list.revoked,
                }
            } else {
                DeviceMsg::Refused {
                    reason: NOT_SHOWN.into(),
                }
            };
            say(&mut send, &reply).await?;
            send.finish().ok();
            return Ok(());
        }
        // Status is read-only: a device that may not send here may still ask.
        if item == DeviceMsg::StatusAsk {
            let host = Arc::clone(&self.host);
            let me = tokio::task::spawn_blocking(move || host.status())
                .await
                .map_err(connection)?;
            say(
                &mut send,
                &DeviceMsg::Status {
                    name: me.name,
                    platform: me.platform,
                    offers: me.offers,
                },
            )
            .await?;
            send.finish().ok();
            return Ok(());
        }
        if !rights.receives_from {
            let reason = "The other device isn't taking files and notes from this one".into();
            say(&mut send, &DeviceMsg::Refused { reason }).await?;
            return Ok(());
        }
        let already = match &item {
            DeviceMsg::Note { id, .. } | DeviceMsg::File { id, .. } => {
                self.host.received_before(them, id)
            }
            _ => false,
        };
        let reply = match item {
            _ if already => DeviceMsg::Done,
            DeviceMsg::Note { id, text } => {
                let host = Arc::clone(&self.host);
                let from = them.to_string();
                match tokio::task::spawn_blocking(move || host.note(&from, id, text))
                    .await
                    .map_err(connection)?
                {
                    Ok(()) => DeviceMsg::Done,
                    Err(err) => DeviceMsg::Refused {
                        reason: err.saved_reason(),
                    },
                }
            }
            DeviceMsg::Message { id, body, reply_to } => {
                let host = Arc::clone(&self.host);
                let from = them.to_string();
                let message = crate::device_chat::PeerMessage { id, body, reply_to };
                match tokio::task::spawn_blocking(move || host.message(&from, message))
                    .await
                    .map_err(connection)?
                {
                    Ok(()) => DeviceMsg::Done,
                    Err(err) => DeviceMsg::Refused {
                        reason: err.saved_reason(),
                    },
                }
            }
            DeviceMsg::File {
                id,
                hash,
                collection,
                name,
                size,
            } => {
                self.receive_file(
                    node, conn, &mut send, them, id, hash, collection, &name, size,
                )
                .await?
            }
            _ => unsupported(),
        };
        say(&mut send, &reply).await?;
        send.finish().ok();
        Ok(())
    }

    /// Fetches an offered file or folder from the sender, ticking while it
    /// moves, and saves it in the received folder under a free name.
    #[allow(clippy::too_many_arguments)]
    async fn receive_file(
        &self,
        node: &Node,
        conn: &Connection,
        send: &mut SendStream,
        them: &str,
        delivery: [u8; 16],
        hash: [u8; 32],
        collection: bool,
        name: &str,
        size: u64,
    ) -> Result<DeviceMsg, RelayError> {
        if size > MAX_FILE_BYTES * 4 {
            return Ok(DeviceMsg::Refused {
                reason: "It's too large".into(),
            });
        }
        let name = clean_offered_name(name, if collection { "folder" } else { "file" });
        let host = Arc::clone(&self.host);
        let (from, file_name) = (them.to_string(), name.clone());
        let id = tokio::task::spawn_blocking(move || host.begin_file(&from, &file_name, size))
            .await
            .map_err(connection)??;
        let hash = Hash::from_bytes(hash);
        let content = if collection {
            HashAndFormat::hash_seq(hash)
        } else {
            HashAndFormat::raw(hash)
        };
        let progress = Arc::new(CodeProgress::default());
        progress.total.store(size, Ordering::SeqCst);
        crate::code::hold(node, content).await?;
        let mut revoked = false;
        // In a block of its own, so the fetch is dropped (and stops) as
        // soon as the loop ends, before anything below waits.
        let fetched = {
            let fetching = crate::code::fetch(node, content, conn.remote_id(), &progress);
            tokio::pin!(fetching);
            // Trust is checked while the file comes, not only at the end: a
            // device revoked meanwhile stops sending at once (the first check
            // is straight away).
            let mut trust_check = tokio::time::interval(TRUST_CHECK);
            let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + TICK, TICK);
            loop {
                tokio::select! {
                    r = &mut fetching => break r,
                    _ = trust_check.tick() => {
                        let host = Arc::clone(&self.host);
                        let from = them.to_string();
                        let still = tokio::task::spawn_blocking(move || host.still_takes_from(&from))
                            .await
                            .map_err(connection)?;
                        if !still {
                            revoked = true;
                            break Err(connection("no longer trusted here"));
                        }
                    }
                    _ = tick.tick() => {
                        let done = progress.done.load(Ordering::SeqCst);
                        self.host.progress(id, done);
                        if say(send, &DeviceMsg::Tick { done }).await.is_err() {
                            break Err(connection("the sender went away"));
                        }
                    }
                }
            }
        };
        // The sender is still connected but what it sent couldn't be had
        // intact: a chunk failed its BLAKE3 check (or the sender no longer
        // holds it). That doesn't heal with a retry.
        if fetched.is_err() && !revoked && conn.close_reason().is_none() {
            let host = Arc::clone(&self.host);
            let from = them.to_string();
            let reason = VERIFICATION_FAILED.to_string();
            tokio::task::spawn_blocking(move || {
                host.finish_file(id, &from, delivery, *hash.as_bytes(), Err(reason))
            })
            .await
            .map_err(connection)?;
            return Ok(DeviceMsg::Refused {
                reason: VERIFICATION_FAILED.to_lowercase(),
            });
        }
        // Revoked (or no longer taking from it) while the file came: what
        // arrived isn't saved, and the store lets go of it.
        let host = Arc::clone(&self.host);
        let from = them.to_string();
        let still = tokio::task::spawn_blocking(move || host.still_takes_from(&from))
            .await
            .map_err(connection)?;
        if !still {
            crate::code::release(node, hash).await;
            let host = Arc::clone(&self.host);
            let from = them.to_string();
            let reason = NO_LONGER_TAKEN.to_string();
            tokio::task::spawn_blocking(move || {
                host.finish_file(id, &from, delivery, *hash.as_bytes(), Err(reason))
            })
            .await
            .map_err(connection)?;
            return Ok(DeviceMsg::Refused {
                reason: NO_LONGER_TAKEN.into(),
            });
        }
        let folder = self.host.folder();
        let saved = match fetched {
            Ok(()) => {
                let out = crate::code::place(node, hash, collection, &folder, &name).await;
                if out.is_ok() {
                    crate::code::release(node, hash).await;
                }
                out
            }
            Err(e) => Err(e),
        };
        let (reply, result) = match &saved {
            Ok(out) => (DeviceMsg::Done, Ok(out.to_string_lossy().into_owned())),
            Err(err) => (
                DeviceMsg::Refused {
                    reason: "It couldn't be received".into(),
                },
                Err(err.saved_reason()),
            ),
        };
        let host = Arc::clone(&self.host);
        let from = them.to_string();
        tokio::task::spawn_blocking(move || {
            host.finish_file(id, &from, delivery, *hash.as_bytes(), result)
        })
        .await
        .map_err(connection)?;
        Ok(reply)
    }
}

/// The answer to an item this build doesn't take: refused, so the sender
/// ends it at once (a connection error would be retried for 30 days).
fn unsupported() -> DeviceMsg {
    DeviceMsg::Refused {
        reason: "This device's Relay doesn't support that yet; update it".into(),
    }
}

/// Why a file from a device revoked while it came isn't saved.
pub(crate) const NOT_ASKED: &str = "The other device doesn't take asks from this one";
pub(crate) const NO_LONGER_TAKEN: &str = "Not saved: that device is no longer trusted here";
pub(crate) const NOT_SHOWN: &str = "The other device doesn't share its devices with this one";

/// Why a file that didn't arrive intact failed, on both sides.
pub(crate) const VERIFICATION_FAILED: &str =
    "Verification failed: what arrived didn't match what was sent";

/// What a send to a trusted device carries.
pub(crate) enum Outgoing {
    Note(String),
    /// A conversation message; it carries its own id.
    Message(crate::device_chat::PeerMessage),
    /// Files and folders as one item, already in the blob store (the
    /// outbox keeps them there).
    Stored(crate::code_send::Offer),
}

/// What a delivered send learned about the receiver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sent {
    /// Its name, as it says it.
    pub name: String,
    /// Where it says it can be reached.
    pub addresses: Vec<String>,
    /// The route the connection used.
    pub route: Option<RouteKind>,
    /// The address it was reached at, when the route is direct.
    pub at: Option<std::net::SocketAddr>,
}

/// Sends one item, `id`, to a trusted device at `to`, on a route `mode`
/// allows. `report` hears how many bytes of a file the receiver has;
/// `stop` ends the send.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send(
    node: &Node,
    to: EndpointAddr,
    mode: ConnectionMode,
    my_name: String,
    id: [u8; 16],
    item: Outgoing,
    report: impl Fn(u64),
    stop: Arc<AtomicBool>,
) -> Result<Sent, RelayError> {
    let them = to.id;
    let conn = tokio::time::timeout(STEP, node.endpoint.connect(to, DEVICE_ALPN))
        .await
        .map_err(|_| RelayError::Unreachable {
            reason: "the other device didn't answer".into(),
        })?
        .map_err(|e| RelayError::Unreachable {
            reason: e.to_string(),
        })?;
    // The path as connected; asked again after the exchange, when iroh
    // may have moved to a better one (or, once closed, has none to show).
    let first_path = crate::resolver::path_of(&conn);
    if let Some(kind) = first_path.map(|(k, _)| k).filter(|k| !mode.allows(*k)) {
        conn.close(0u32.into(), b"route not allowed");
        return Err(RelayError::Unreachable {
            reason: format!(
                "the only route found was {}, which the Connections setting doesn't allow",
                kind.label().to_lowercase()
            ),
        });
    }
    let result = exchange(node, &conn, them, my_name, id, item, report, &stop)
        .await
        .map(|(name, addresses)| {
            let path = crate::resolver::path_of(&conn).or(first_path);
            Sent {
                name,
                addresses,
                route: path.map(|(kind, _)| kind),
                at: path.and_then(|(_, at)| at),
            }
        });
    let result = match result {
        // The listener closed the connection without a word: it doesn't
        // trust this device.
        Err(RelayError::Connection { .. }) if matches!(conn.close_reason(), Some(iroh::endpoint::ConnectionError::ApplicationClosed(ref c)) if c.error_code == NOT_TRUSTED.into()) => {
            Err(RelayError::Refused)
        }
        other => other,
    };
    conn.close(0u32.into(), b"bye");
    result
}

/// Asks a trusted device at `to` what it says about itself (brief PR 5).
pub(crate) async fn ask_status(
    node: &Node,
    to: EndpointAddr,
    my_name: String,
) -> Result<StatusReport, RelayError> {
    let conn = connect(node, to).await?;
    let result = async {
        let (mut send, mut recv) = hello(node, &conn, my_name).await?;
        say(&mut send, &DeviceMsg::StatusAsk).await?;
        let answer = hear(&mut recv, STEP).await;
        send.finish().ok();
        match answer? {
            DeviceMsg::Status {
                name,
                platform,
                offers,
            } => Ok(StatusReport {
                name,
                platform: clean_label(&platform),
                offers: offers
                    .iter()
                    .map(|o| clean_label(o))
                    .filter(|o| !o.is_empty())
                    .take(32)
                    .collect(),
            }),
            DeviceMsg::Refused { reason } => Err(RelayError::Declined { reason }),
            _ => Err(connection("unexpected answer")),
        }
    }
    .await;
    let result = not_trusted(&conn, result);
    conn.close(0u32.into(), b"bye");
    result
}

/// The most devices read from one introducer's list.
const MOST_LISTED: usize = 256;

/// Asks a trusted device at `to` which devices it trusts and has revoked
/// (My Devices, step 1).
pub(crate) async fn ask_devices(
    node: &Node,
    to: EndpointAddr,
    my_name: String,
) -> Result<crate::trust::DeviceList, RelayError> {
    let conn = connect(node, to).await?;
    let result = async {
        let (mut send, mut recv) = hello(node, &conn, my_name).await?;
        say(&mut send, &DeviceMsg::DevicesAsk).await?;
        let answer = hear(&mut recv, STEP).await;
        send.finish().ok();
        match answer? {
            DeviceMsg::DeviceList { trusted, revoked } => {
                let id_ok = |id: &str| {
                    !id.is_empty()
                        && id.len() <= 128
                        && id.chars().all(|c| c.is_ascii_alphanumeric())
                };
                Ok(crate::trust::DeviceList {
                    trusted: trusted
                        .into_iter()
                        .filter(|(id, _)| id_ok(id))
                        .take(MOST_LISTED)
                        .map(|(id, name)| {
                            let name = clean_offered_name(&name, "Unnamed device");
                            (id, name)
                        })
                        .collect(),
                    revoked: revoked
                        .into_iter()
                        .filter(|id| id_ok(id))
                        .take(MOST_LISTED)
                        .collect(),
                })
            }
            DeviceMsg::Refused { reason } => Err(RelayError::Declined { reason }),
            _ => Err(connection("unexpected answer")),
        }
    }
    .await;
    let result = not_trusted(&conn, result);
    conn.close(0u32.into(), b"bye");
    result
}

/// Where the answering model writes: its text goes to the sending loop,
/// and it stops when the asker says so.
struct Pipe {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
    stop: Arc<AtomicBool>,
}

impl TextOut for Pipe {
    fn push(&self, delta: &str) -> bool {
        !self.stop.load(Ordering::SeqCst) && self.tx.send(delta.to_string()).is_ok()
    }

    fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// Connects to a trusted device at `to` and says Hello.
async fn hello(
    node: &Node,
    conn: &Connection,
    my_name: String,
) -> Result<(SendStream, RecvStream), RelayError> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(connection)?;
    say(
        &mut send,
        &DeviceMsg::Hello {
            version: VERSION,
            name: my_name,
            addresses: node.addresses(),
        },
    )
    .await?;
    match hear(&mut recv, STEP).await? {
        DeviceMsg::Hello { .. } => Ok((send, recv)),
        DeviceMsg::Refused { reason } => Err(RelayError::Declined { reason }),
        _ => Err(connection("no hello")),
    }
}

async fn connect(node: &Node, to: EndpointAddr) -> Result<Connection, RelayError> {
    tokio::time::timeout(STEP, node.endpoint.connect(to, DEVICE_ALPN))
        .await
        .map_err(|_| RelayError::Unreachable {
            reason: "the other device didn't answer".into(),
        })?
        .map_err(|e| RelayError::Unreachable {
            reason: e.to_string(),
        })
}

/// A connection error, read again: when the listener closed without a
/// word, it doesn't trust this device.
fn not_trusted<T>(conn: &Connection, result: Result<T, RelayError>) -> Result<T, RelayError> {
    match result {
        Err(RelayError::Connection { .. }) if matches!(conn.close_reason(), Some(iroh::endpoint::ConnectionError::ApplicationClosed(ref c)) if c.error_code == NOT_TRUSTED.into()) => {
            Err(RelayError::Refused)
        }
        other => other,
    }
}

/// Asks a trusted device's model. Its text goes to `pieces` as it comes;
/// once `stop` is set the other device is told to stop. The outer result
/// is the connection's, the inner the model's (`Ok` with its name).
pub(crate) async fn ask(
    node: &Node,
    to: EndpointAddr,
    my_name: String,
    request: ChatRequest,
    pieces: std::sync::mpsc::Sender<String>,
    stop: Arc<AtomicBool>,
) -> Result<Result<String, AiError>, RelayError> {
    let conn = connect(node, to).await?;
    let result = async {
        let (mut send, mut recv) = hello(node, &conn, my_name).await?;
        say(&mut send, &DeviceMsg::Ask { request }).await?;
        // Says Stop once asked to, while the loop below listens.
        let stopper = tokio::spawn({
            let stop = Arc::clone(&stop);
            async move {
                while !stop.load(Ordering::SeqCst) {
                    tokio::time::sleep(STOP_CHECK).await;
                }
                let _ = say(&mut send, &DeviceMsg::Stop).await;
                send
            }
        });
        let answer = loop {
            match hear(&mut recv, ANSWER_GAP).await {
                Ok(DeviceMsg::Delta { text }) => {
                    if pieces.send(text).is_err() {
                        stop.store(true, Ordering::SeqCst);
                    }
                }
                Ok(DeviceMsg::Answered { .. } | DeviceMsg::Failed { .. })
                    if stop.load(Ordering::SeqCst) =>
                {
                    break Ok(Err(AiError::Stopped))
                }
                Ok(DeviceMsg::Answered { model }) => break Ok(Ok(model)),
                Ok(DeviceMsg::Failed { error }) => break Ok(Err(error)),
                Ok(DeviceMsg::Refused { reason }) => break Err(RelayError::Declined { reason }),
                Ok(_) => break Err(connection("expected an answer")),
                Err(_) if stop.load(Ordering::SeqCst) => break Ok(Err(AiError::Stopped)),
                Err(err) => break Err(err),
            }
        };
        stopper.abort();
        answer
    }
    .await;
    let result = not_trusted(&conn, result);
    conn.close(0u32.into(), b"bye");
    result
}

/// A short word from another device (a platform or an offer) made safe to
/// show: letters, digits, '-', '_' and '.', at most 32 characters.
pub(crate) fn clean_label(label: &str) -> String {
    label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(32)
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn exchange(
    node: &Node,
    conn: &Connection,
    them: EndpointId,
    my_name: String,
    id: [u8; 16],
    item: Outgoing,
    report: impl Fn(u64),
    stop: &AtomicBool,
) -> Result<(String, Vec<String>), RelayError> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(connection)?;
    say(
        &mut send,
        &DeviceMsg::Hello {
            version: VERSION,
            name: my_name,
            addresses: node.addresses(),
        },
    )
    .await?;
    let (their_name, their_addresses) = match hear(&mut recv, STEP).await? {
        DeviceMsg::Hello {
            name, addresses, ..
        } => (name, crate::resolver::clean(&addresses)),
        DeviceMsg::Refused { reason } => return Err(RelayError::Declined { reason }),
        _ => return Err(connection("no hello")),
    };
    let ending = match item {
        Outgoing::Note(text) => {
            say(&mut send, &DeviceMsg::Note { id, text }).await?;
            hear(&mut recv, STEP).await?
        }
        Outgoing::Message(m) => {
            let msg = DeviceMsg::Message {
                id: m.id,
                body: m.body,
                reply_to: m.reply_to,
            };
            say(&mut send, &msg).await?;
            hear(&mut recv, STEP).await?
        }
        Outgoing::Stored(offer) => {
            lock(&node.device_allowed)
                .entry(them)
                .or_default()
                .extend(offer.hashes.iter().copied());
            let ending = offer_file(&mut send, &mut recv, id, &offer, &report, stop).await;
            if let Some(allowed) = lock(&node.device_allowed).get_mut(&them) {
                allowed.retain(|h| !offer.hashes.contains(h));
            }
            ending?
        }
    };
    send.finish().ok();
    match ending {
        DeviceMsg::Done => Ok((their_name, their_addresses)),
        DeviceMsg::Refused { reason } => Err(RelayError::Declined { reason }),
        _ => Err(connection("unexpected answer")),
    }
}

async fn offer_file(
    send: &mut SendStream,
    recv: &mut RecvStream,
    id: [u8; 16],
    offer: &crate::code_send::Offer,
    report: &impl Fn(u64),
    stop: &AtomicBool,
) -> Result<DeviceMsg, RelayError> {
    say(
        send,
        &DeviceMsg::File {
            id,
            hash: *offer.hash.as_bytes(),
            collection: offer.collection,
            name: offer.name.clone(),
            size: offer.size,
        },
    )
    .await?;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(RelayError::Stopped {
                kind: forge_app::ErrorKind::Cancelled,
            });
        }
        match hear(recv, SILENCE).await? {
            DeviceMsg::Tick { done } => report(done),
            other => return Ok(other),
        }
    }
}

/// The dial address of a trusted device: its key, and where the resolver
/// says to look for it.
pub(crate) fn endpoint_addr(
    endpoint_id: &str,
    addresses: &[std::net::SocketAddr],
) -> Result<EndpointAddr, RelayError> {
    let id: EndpointId = endpoint_id.parse().map_err(|_| RelayError::UnknownPeer {
        id: endpoint_id.to_string(),
    })?;
    let addrs = addresses.iter().copied().map(iroh::TransportAddr::Ip);
    Ok(EndpointAddr::from_parts(id, addrs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// One of each message, as committed bytes. A change that breaks one
    /// breaks every Relay already on a device: bump `VERSION` instead.
    fn fixtures() -> Vec<(DeviceMsg, &'static str)> {
        vec![
            (
                DeviceMsg::Hello {
                    version: 1,
                    name: "Laptop".into(),
                    addresses: vec![],
                },
                include_str!("../fixtures/device-1/hello.hex"),
            ),
            (
                DeviceMsg::Hello {
                    version: 1,
                    name: "Laptop".into(),
                    addresses: vec!["100.72.135.124:4000".into()],
                },
                include_str!("../fixtures/device-1/hello-addresses.hex"),
            ),
            (
                DeviceMsg::Note {
                    id: [7; 16],
                    text: "hi".into(),
                },
                include_str!("../fixtures/device-1/note.hex"),
            ),
            (
                DeviceMsg::File {
                    id: [1; 16],
                    hash: [2; 32],
                    collection: true,
                    name: "Trip".into(),
                    size: 24,
                },
                include_str!("../fixtures/device-1/file.hex"),
            ),
            (
                DeviceMsg::Tick { done: 1024 },
                include_str!("../fixtures/device-1/tick.hex"),
            ),
            (
                DeviceMsg::Done,
                include_str!("../fixtures/device-1/done.hex"),
            ),
            (
                DeviceMsg::Refused {
                    reason: "no".into(),
                },
                include_str!("../fixtures/device-1/refused.hex"),
            ),
            (
                DeviceMsg::Message {
                    id: "m1".into(),
                    body: "hi".into(),
                    reply_to: None,
                },
                include_str!("../fixtures/device-1/message.hex"),
            ),
            (
                DeviceMsg::StatusAsk,
                include_str!("../fixtures/device-1/status-ask.hex"),
            ),
            (
                DeviceMsg::Status {
                    name: "Laptop".into(),
                    platform: "linux".into(),
                    offers: vec!["notes".into(), "files".into()],
                },
                include_str!("../fixtures/device-1/status.hex"),
            ),
        ]
    }

    #[test]
    fn messages_match_their_golden_bytes() {
        for (msg, golden) in fixtures() {
            let golden = golden.trim();
            assert_eq!(hex(&msg.encode()), golden, "{msg:?}");
            let bytes: Vec<u8> = (0..golden.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&golden[i..i + 2], 16).unwrap())
                .collect();
            assert_eq!(DeviceMsg::decode(&bytes).unwrap(), msg);
        }
    }

    /// What lets a device without M2 PR 6 read a Hello with addresses: a
    /// field it doesn't know is skipped, not an error.
    #[test]
    fn unknown_hello_fields_are_ignored() {
        let later = "a16548656c6c6fa36776657273696f6e01646e616d65664c6170746f70656c61746572f5";
        let bytes: Vec<u8> = (0..later.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&later[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(
            DeviceMsg::decode(&bytes).unwrap(),
            DeviceMsg::Hello {
                version: 1,
                name: "Laptop".into(),
                addresses: vec![],
            }
        );
    }

    #[test]
    fn labels_from_another_device_are_cleaned() {
        assert_eq!(clean_label("ios"), "ios");
        assert_eq!(clean_label("ai.inference"), "ai.inference");
        assert_eq!(clean_label("files\u{202E}exe <b>"), "filesexeb");
        assert_eq!(clean_label(&"x".repeat(100)).len(), 32);
    }

    /// A newer build's item, which this build can't read, is refused
    /// with a reason, never dropped with the connection (which the outbox
    /// would retry for 30 days).
    #[test]
    fn an_item_this_build_cant_read_is_refused_not_dropped() {
        #[derive(Serialize)]
        enum Newer {
            Teleport { to: String },
        }
        let dir = tempfile::tempdir().unwrap();
        crate::code::TOKIO.block_on(async {
            let listener = test_node(dir.path(), "listener", Arc::new(TrustsAll)).await;
            let sender = test_node(dir.path(), "sender", Arc::new(NoDevices)).await;
            let conn = connect(&sender, listener.endpoint.addr()).await.unwrap();
            let (mut send, mut recv) = hello(&sender, &conn, "sender".into()).await.unwrap();
            let mut newer = Vec::new();
            ciborium::into_writer(&Newer::Teleport { to: "moon".into() }, &mut newer).unwrap();
            put(&mut send, &newer).await.unwrap();
            let answer = hear(&mut recv, STEP).await.unwrap();
            assert_eq!(answer, unsupported());
            conn.close(0u32.into(), b"bye");
            // A message it reads, but doesn't take on this protocol's item
            // step, is refused the same way.
            let conn = connect(&sender, listener.endpoint.addr()).await.unwrap();
            let (mut send, mut recv) = hello(&sender, &conn, "sender".into()).await.unwrap();
            say(&mut send, &DeviceMsg::Done).await.unwrap();
            assert_eq!(hear(&mut recv, STEP).await.unwrap(), unsupported());
            conn.close(0u32.into(), b"bye");
            sender.shutdown().await;
            listener.shutdown().await;
        });
    }

    /// Takes from anyone when a connection opens, but no longer by the
    /// time a file has arrived (it was revoked meanwhile); keeps how each
    /// file ended.
    /// A host that stops taking from the sender after `.1` trust checks.
    struct RevokedMeanwhile(
        std::sync::Mutex<Vec<Result<String, String>>>,
        std::sync::atomic::AtomicUsize,
    );

    impl DeviceInbound for RevokedMeanwhile {
        fn rights(&self, from: &str) -> Option<Rights> {
            TrustsAll.rights(from)
        }
        fn still_takes_from(&self, _: &str) -> bool {
            self.1
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
        }
        fn my_name(&self) -> String {
            TrustsAll.my_name()
        }
        fn status(&self) -> StatusReport {
            StatusReport::default()
        }
        fn reached(&self, _: &str) {}
        fn folder(&self) -> PathBuf {
            PathBuf::from("/nonexistent")
        }
        fn received_before(&self, _: &str, _: &[u8; 16]) -> bool {
            false
        }
        fn note(&self, _: &str, _: [u8; 16], _: String) -> Result<(), RelayError> {
            Ok(())
        }
        fn message(&self, _: &str, _: crate::device_chat::PeerMessage) -> Result<(), RelayError> {
            Ok(())
        }
        fn begin_file(&self, _: &str, _: &str, _: u64) -> Result<u64, RelayError> {
            Ok(1)
        }
        fn progress(&self, _: u64, _: u64) {}
        fn finish_file(
            &self,
            _: u64,
            _: &str,
            _: [u8; 16],
            _: [u8; 32],
            result: Result<String, String>,
        ) {
            lock(&self.0).push(result);
        }
    }

    /// A device revoked while its file arrives: the file isn't saved, and
    /// both sides say why.
    #[test]
    fn a_file_from_a_device_revoked_meanwhile_is_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(RevokedMeanwhile(
            std::sync::Mutex::new(Vec::new()),
            std::sync::atomic::AtomicUsize::new(0),
        ));
        crate::code::TOKIO.block_on(async {
            let listener = test_node(dir.path(), "listener", Arc::clone(&host) as _).await;
            let sender = test_node(dir.path(), "sender", Arc::new(NoDevices)).await;
            let file = dir.path().join("map.txt");
            std::fs::write(&file, b"north").unwrap();
            let (offer, _tags) =
                crate::code_send::prepare(&[file], "map.txt".into(), &sender, true)
                    .await
                    .unwrap();
            let sent = send(
                &sender,
                listener.endpoint.addr(),
                ConnectionMode::Hybrid,
                "sender".into(),
                [7; 16],
                Outgoing::Stored(offer),
                |_| {},
                Arc::new(AtomicBool::new(false)),
            )
            .await;
            assert!(
                matches!(&sent, Err(RelayError::Declined { reason }) if reason == NO_LONGER_TAKEN),
                "{sent:?}"
            );
            assert_eq!(*lock(&host.0), [Err(NO_LONGER_TAKEN.to_string())]);
            sender.shutdown().await;
            listener.shutdown().await;
        });
    }

    /// Revoked while a large file comes: it stops at once rather than
    /// running to the end and being refused at the save (found on the
    /// phone, 2026-11-21: 504 of 524 MB crossed after the revoke).
    #[test]
    fn a_device_revoked_meanwhile_stops_sending_at_once() {
        let dir = tempfile::tempdir().unwrap();
        // Trusted for the first two checks (the one at the start and the next),
        // so the file is on its way when trust goes.
        let host = Arc::new(RevokedMeanwhile(
            std::sync::Mutex::new(Vec::new()),
            std::sync::atomic::AtomicUsize::new(2),
        ));
        crate::code::TOKIO.block_on(async {
            let listener = test_node(dir.path(), "listener", Arc::clone(&host) as _).await;
            let sender = test_node(dir.path(), "sender", Arc::new(NoDevices)).await;
            let file = dir.path().join("big.bin");
            let mut data = vec![0u8; 64 * 1024 * 1024];
            for (i, b) in data.iter_mut().enumerate() {
                *b = (i.wrapping_mul(2654435761) >> 13) as u8;
            }
            std::fs::write(&file, &data).unwrap();
            let (offer, _tags) =
                crate::code_send::prepare(&[file], "big.bin".into(), &sender, false)
                    .await
                    .unwrap();
            let hash = offer.hash;
            let sent = send(
                &sender,
                listener.endpoint.addr(),
                ConnectionMode::Hybrid,
                "sender".into(),
                [8; 16],
                Outgoing::Stored(offer),
                |_| {},
                Arc::new(AtomicBool::new(false)),
            )
            .await;
            assert!(
                matches!(&sent, Err(RelayError::Declined { reason }) if reason == NO_LONGER_TAKEN),
                "{sent:?}"
            );
            let status = listener.store.blobs().status(hash).await.unwrap();
            assert!(
                !matches!(status, iroh_blobs::api::blobs::BlobStatus::Complete { .. }),
                "the whole file came anyway: {status:?}"
            );
            sender.shutdown().await;
            listener.shutdown().await;
        });
    }

    async fn test_node(
        dir: &std::path::Path,
        name: &str,
        host: Arc<dyn DeviceInbound>,
    ) -> Arc<Node> {
        let key_dir = dir.join(name);
        let link = NodeLink::default();
        let node = Arc::new(
            Node::start(
                &key_dir,
                &key_dir.join("store"),
                host,
                Arc::clone(&link),
                &crate::resolver::RelayServers::default(),
            )
            .await
            .unwrap(),
        );
        let _ = link.set(Arc::downgrade(&node));
        node
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(DeviceMsg::decode(b"").is_err());
        assert!(DeviceMsg::decode(&[0xff; 40]).is_err());
    }

    #[test]
    fn offered_names_are_cleaned() {
        assert_eq!(clean_offered_name("../../secret", "file"), "secret");
        assert_eq!(clean_offered_name("a\\b\\c.txt", "file"), "c.txt");
        assert_eq!(
            clean_offered_name("photo\u{202E}gpj.exe", "file"),
            "photogpj.exe"
        );
        assert_eq!(clean_offered_name("bell\u{7}.txt", "file"), "bell.txt");
        assert_eq!(clean_offered_name("..", "folder"), "folder");
        assert_eq!(clean_offered_name("  ", "file"), "file");
        assert!(clean_offered_name(&"é".repeat(300), "file").len() <= 255);
    }
}
