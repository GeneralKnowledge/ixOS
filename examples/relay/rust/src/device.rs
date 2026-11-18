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

use crate::code::{connection, get_within, put, CodeProgress, STEP};
use crate::model::{RelayError, MAX_FILE_BYTES};
use crate::node::{lock, Node};
use crate::resolver::{route_of, ConnectionMode, RouteKind};

pub(crate) const DEVICE_ALPN: &[u8] = b"forge-relay/device/1";
/// The protocol's version inside `relay/device/1`.
pub(crate) const VERSION: u16 = 1;
/// How often the receiver says how far it has got.
const TICK: Duration = Duration::from_secs(2);
/// How long the sender waits without a tick before giving up.
const SILENCE: Duration = Duration::from_secs(30);
/// The close code for a device that isn't trusted.
const NOT_TRUSTED: u32 = 403;

/// Everything said on `relay/device/1`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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
    /// This device's name, for the reply to Hello.
    fn my_name(&self) -> String;
    /// A trusted device said where it can be reached, and connected over
    /// `route`.
    fn met(&self, _from: &str, _addresses: Vec<String>, _route: Option<RouteKind>) {}
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
    /// `from` remembered), or failed with the reason.
    fn finish_file(&self, id: u64, from: &str, delivery: [u8; 16], result: Result<String, String>);
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
    fn finish_file(&self, _: u64, _: &str, _: [u8; 16], _: Result<String, String>) {}
}

/// What a trusted device may do here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rights {
    pub receives_from: bool,
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
        let item = hear(&mut recv, STEP).await?;
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
            _ => return Err(connection("expected a note or a file")),
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
        let fetching = crate::code::fetch(node, content, conn.remote_id(), &progress);
        tokio::pin!(fetching);
        let fetched = loop {
            tokio::select! {
                r = &mut fetching => break r,
                _ = tokio::time::sleep(TICK) => {
                    let done = progress.done.load(Ordering::SeqCst);
                    self.host.progress(id, done);
                    if say(send, &DeviceMsg::Tick { done }).await.is_err() {
                        break Err(connection("the sender went away"));
                    }
                }
            }
        };
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
        tokio::task::spawn_blocking(move || host.finish_file(id, &from, delivery, result))
            .await
            .map_err(connection)?;
        Ok(reply)
    }
}

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
    if let Some(kind) = route_of(&conn).filter(|k| !mode.allows(*k)) {
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
            let path = crate::resolver::path_of(&conn);
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
