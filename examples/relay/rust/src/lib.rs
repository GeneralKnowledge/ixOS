//! Relay: send notes and files between nearby devices, encrypted, after
//! pairing once by QR code.
//!
//! The app's whole Rust side. The wire format is in [`wire`], one
//! connection end to end in [`link`], the saved record in `record`; this
//! file is the API the SwiftUI app calls, built on the application runtime
//! (`forge_app`):
//!
//! - **Pairing.** One device listens and shows `create_invitation`'s code
//!   as a QR code; the other scans it and calls `pair`. The code carries a
//!   one-time secret that never crosses the network: the scanner connects,
//!   and the Noise handshake succeeds only if both hold it. Both then keep
//!   it as the pair's secret, and every later connection is keyed by it.
//! - **Sending** (`send_note`, `send_file`) dials the paired device at the
//!   addresses the app found with Bonjour (`NWBrowser`, service
//!   `_forgerelay._tcp`), or where it was last reached. It is finishing
//!   work: it keeps going when the app leaves the screen, while iOS allows.
//! - **Receiving.** `start_listening` opens a TCP port (the app advertises
//!   it with Bonjour) and takes notes and files from paired devices. A file
//!   is kept only if its SHA-256 matches what the sender announced.
//! - The `relay` command (`--features cli`) is the same code on Linux, so
//!   one iPhone and a laptop are enough to try it.

pub mod link;
mod model;
mod record;
pub mod wire;

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use forge_app::{Cancellation, DataDirectory, Document, Published, Runtime};
use forge_runtime::{lock, now_ms};
use forge_storage::FileStore;

use link::{Inbound, Link, Me, Outgoing, Them};
pub use model::{
    relay_error_kind, Direction, Identity, Invitation, Item, Peer, RelayError, Transfer,
    TransferStatus, MAX_FILE_BYTES, MAX_NAME_CHARS, MAX_NOTE_CHARS,
};
use record::{Record, SavedItem, SavedStatus};
use wire::{Code, Intro, Offered, Purpose};

uniffi::setup_scaffolding!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes to the same
// log as Rust.
forge_runtime::log_export!();

const SAVE_FILE: &str = "relay.json";
/// How long a pairing code stays valid.
pub const INVITATION_MS: i64 = 5 * 60 * 1000;
/// The Bonjour service type the app advertises and browses.
pub const SERVICE_TYPE: &str = "_forgerelay._tcp";

/// What the command thread owns.
struct State {
    record: Document<Record>,
    inbox: FileStore,
    published: Arc<Published<Record>>,
}

impl State {
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Record) -> Result<T, RelayError>,
    ) -> Result<T, RelayError> {
        let value = self.record.change(edit)?;
        self.published.set(self.record.value().clone());
        Ok(value)
    }
}

/// The open invitation: its secret and when it expires.
type Pending = Option<([u8; 32], i64)>;

struct Listening {
    stop: Arc<AtomicBool>,
    port: u16,
}

/// The app's core. The Swift app opens one per process and shares it.
#[derive(uniffi::Object)]
pub struct RelayCore {
    rt: Runtime<State>,
    record: Arc<Published<Record>>,
    inbox: PathBuf,
    invitation: Mutex<Pending>,
    progress: Mutex<HashMap<u64, u64>>,
    listening: Mutex<Option<Listening>>,
    me: Weak<RelayCore>,
}

#[uniffi::export]
impl RelayCore {
    /// Opens Relay's data in `data_directory` (created if missing), holding
    /// the directory so no second core can write to it. The first open
    /// makes this device's identity, named `device_name` (the person can
    /// rename it).
    #[uniffi::constructor]
    pub fn new(data_directory: String, device_name: String) -> Result<Arc<Self>, RelayError> {
        forge_runtime::logging::init();
        let directory = DataDirectory::open(&data_directory)?;
        let mut record =
            Document::<Record>::open(directory.path().join(SAVE_FILE), record::VERSION)?;
        if record.value().device_id.is_empty() {
            let name = model::clean_name(&device_name).unwrap_or_else(|_| "Relay".into());
            record.change(|r| {
                r.device_id = wire::hex(&forge_devproto::new_secret()[..16]);
                r.name = name;
                Ok::<_, RelayError>(())
            })?;
        }
        if record
            .value()
            .transfers
            .iter()
            .any(|t| t.status == SavedStatus::InProgress)
        {
            record.change(|r| {
                r.interrupted();
                Ok::<_, RelayError>(())
            })?;
        }
        let inbox = FileStore::new(directory.path().join("inbox")).with_max_bytes(MAX_FILE_BYTES);
        inbox.sweep_temporaries();
        let published = Arc::new(Published::new(record.value().clone()));
        let inbox_root = inbox.root().to_path_buf();
        let state = State {
            record,
            inbox,
            published: Arc::clone(&published),
        };
        Ok(Arc::new_cyclic(|me| Self {
            rt: Runtime::with_directory("relay", directory, state),
            record: published,
            inbox: inbox_root,
            invitation: Mutex::new(None),
            progress: Mutex::new(HashMap::new()),
            listening: Mutex::new(None),
            me: me.clone(),
        }))
    }

    /// This device, as others see it.
    pub fn identity(&self) -> Identity {
        let r = self.record.get();
        Identity {
            device_id: r.device_id.clone(),
            name: r.name.clone(),
        }
    }

    pub async fn rename_device(&self, name: String) -> Result<(), RelayError> {
        self.rt
            .command("rename-device", move |state| {
                let name = model::clean_name(&name)?;
                state.change(|r| {
                    r.name = name;
                    Ok(())
                })
            })
            .await
    }

    /// Paired devices, by name.
    pub fn peers(&self) -> Vec<Peer> {
        self.record.get().peers()
    }

    /// Unpairs a device: its secret is deleted, so it can't connect again
    /// until it pairs again. Its history stays.
    pub async fn forget_peer(&self, device_id: String) -> Result<(), RelayError> {
        self.rt
            .command("forget-peer", move |state| {
                state.change(|r| {
                    let before = r.peers.len();
                    r.peers.retain(|p| p.device_id != device_id);
                    if r.peers.len() == before {
                        return Err(RelayError::UnknownPeer { id: device_id });
                    }
                    Ok(())
                })
            })
            .await
    }

    /// Everything sent and received, newest first, with progress for what
    /// is still going. Never waits for a command.
    pub fn transfers(&self) -> Vec<Transfer> {
        let progress = lock(&self.progress).clone();
        let inbox = FileStore::new(&self.inbox);
        self.record.get().transfers(
            |name| inbox.get(name).map(|p| p.to_string_lossy().into_owned()),
            |id| progress.get(&id).copied().unwrap_or(0),
        )
    }

    /// Removes a history entry and, for a received file, the file.
    pub async fn remove_transfer(&self, id: u64) -> Result<(), RelayError> {
        self.rt
            .command("remove-transfer", move |state| {
                let gone = state.change(|r| {
                    let i = r
                        .transfers
                        .iter()
                        .position(|t| t.id == id)
                        .ok_or(RelayError::NotFound { id })?;
                    let t = r.transfers.remove(i);
                    let still_used = |name: &str| {
                        r.transfers.iter().any(|o| {
                            !o.sent
                                && matches!(&o.item, SavedItem::File { file: Some(f), .. } if f == name)
                        })
                    };
                    Ok(match t.item {
                        SavedItem::File { file: Some(f), .. } if !t.sent && !still_used(&f) => {
                            Some(f)
                        }
                        _ => None,
                    })
                })?;
                if let Some(name) = gone {
                    state.inbox.remove(&name);
                }
                Ok(())
            })
            .await
    }

    /// Opens a TCP port for paired devices (and pairing) on every
    /// interface; returns the port. `port` 0 takes any free one. Advertise
    /// it with Bonjour as [`SERVICE_TYPE`]. Stop it when the app leaves the
    /// screen: iOS suspends it anyway.
    pub fn start_listening(&self, port: u16) -> Result<u16, RelayError> {
        let mut listening = lock(&self.listening);
        if listening.is_some() {
            return Err(RelayError::AlreadyListening);
        }
        let listener =
            TcpListener::bind(("0.0.0.0", port)).map_err(|err| RelayError::Unreachable {
                reason: format!("can't open port {port}: {err}"),
            })?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let me = self.me.clone();
        let flag = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("relay-listen".into())
            .spawn(move || serve(listener, me, flag))
            .map_err(|err| RelayError::Storage {
                reason: format!("start listening: {err}"),
            })?;
        *listening = Some(Listening { stop, port });
        Ok(port)
    }

    /// Closes the port. Transfers already arriving finish.
    pub fn stop_listening(&self) {
        if let Some(l) = lock(&self.listening).take() {
            l.stop.store(true, Ordering::SeqCst);
        }
    }

    /// The port Relay listens on, if it does.
    pub fn listening_port(&self) -> Option<u16> {
        lock(&self.listening).as_ref().map(|l| l.port)
    }

    /// A pairing code for the other device to scan, valid once for five
    /// minutes. `hosts` are this device's addresses on the local network
    /// (the app reads them from `NWPathMonitor`); the listening port is
    /// added. Start listening first.
    pub fn create_invitation(&self, hosts: Vec<String>) -> Result<Invitation, RelayError> {
        let port = self.listening_port().ok_or(RelayError::Unreachable {
            reason: "Relay isn't listening".into(),
        })?;
        let addresses: Vec<String> = hosts
            .iter()
            .map(|h| h.trim())
            .filter(|h| !h.is_empty())
            .map(|h| {
                if h.contains(':') && !h.starts_with('[') {
                    format!("[{h}]:{port}") // IPv6
                } else {
                    format!("{h}:{port}")
                }
            })
            .collect();
        if addresses.is_empty() {
            return Err(RelayError::Unreachable {
                reason: "this device has no network address".into(),
            });
        }
        let secret = forge_devproto::new_secret();
        let expires_ms = now_ms() + INVITATION_MS;
        *lock(&self.invitation) = Some((secret, expires_ms));
        let me = self.identity();
        let code = Code {
            device_id: me.device_id,
            name: me.name,
            secret,
            addresses,
        };
        Ok(Invitation {
            code: code.encode(),
            expires_ms,
        })
    }

    /// Withdraws the open pairing code.
    pub fn cancel_invitation(&self) {
        lock(&self.invitation).take();
    }

    /// Pairs with the device whose code was scanned: connects to it, proves
    /// both hold the code's secret, and keeps the other device. Expires
    /// with the code (the other device refuses it).
    pub async fn pair(
        &self,
        code: String,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Peer, RelayError> {
        let code = Code::parse(&code)?;
        let me = self.me_now();
        if code.device_id == me.device_id {
            return Err(RelayError::BadCode);
        }
        let work = self.rt.foreground(cancellation.as_deref());
        let (them, address) = {
            let code = code.clone();
            self.rt
                .run("pair", work, move |_| {
                    let link = Link::dial(
                        &code.addresses,
                        &me,
                        Purpose::Pair,
                        &code.secret,
                        &code.device_id,
                    )?;
                    let result = (link.them.clone(), link.address.clone());
                    link.close();
                    Ok::<_, RelayError>(result)
                })
                .await?
        };
        self.remember_pair(&them, &code.secret, Some(address))
            .await?;
        self.peers()
            .into_iter()
            .find(|p| p.device_id == them.device_id)
            .ok_or(RelayError::UnknownPeer { id: them.device_id })
    }

    /// Sends a note to a paired device at `addresses` ("host:port", from
    /// Bonjour), or where it was last reached when there are none.
    pub async fn send_note(
        &self,
        device_id: String,
        text: String,
        addresses: Vec<String>,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Transfer, RelayError> {
        let text = model::clean_note(&text)?;
        let item = SavedItem::Note { text: text.clone() };
        self.send(
            device_id,
            item,
            Payload::Note(text),
            addresses,
            cancellation,
        )
        .await
    }

    /// Sends the file at `path` to a paired device. The file must stay put
    /// until the transfer ends.
    pub async fn send_file(
        &self,
        device_id: String,
        path: String,
        addresses: Vec<String>,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Transfer, RelayError> {
        let source = PathBuf::from(&path);
        let size = std::fs::metadata(&source)
            .map_err(|err| RelayError::File {
                reason: err.to_string(),
            })?
            .len();
        if size > MAX_FILE_BYTES {
            return Err(RelayError::FileTooLarge);
        }
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let item = SavedItem::File {
            name: name.clone(),
            size,
            file: Some(path),
        };
        self.send(
            device_id,
            item,
            Payload::File { source, name, size },
            addresses,
            cancellation,
        )
        .await
    }
}

/// What `send` carries, owned for the work thread.
enum Payload {
    Note(String),
    File {
        source: PathBuf,
        name: String,
        size: u64,
    },
}

impl RelayCore {
    fn me_now(&self) -> Me {
        let id = self.identity();
        Me {
            device_id: id.device_id,
            name: id.name,
        }
    }

    async fn remember_pair(
        &self,
        them: &Them,
        secret: &[u8; 32],
        address: Option<String>,
    ) -> Result<(), RelayError> {
        let them = them.clone();
        let secret = *secret;
        self.rt
            .command("pair", move |state| {
                state.change(|r| {
                    r.pair(&them.device_id, &them.name, &secret, address, now_ms());
                    Ok(())
                })
            })
            .await
    }

    async fn send(
        &self,
        device_id: String,
        item: SavedItem,
        payload: Payload,
        addresses: Vec<String>,
        cancellation: Option<Arc<Cancellation>>,
    ) -> Result<Transfer, RelayError> {
        let record = self.record.get();
        let peer = record
            .peer(&device_id)
            .ok_or_else(|| RelayError::UnknownPeer {
                id: device_id.clone(),
            })?;
        let secret = record.secret(&device_id).ok_or(RelayError::Refused)?;
        let addresses = if addresses.is_empty() {
            peer.last_address.iter().cloned().collect()
        } else {
            addresses
        };
        if addresses.is_empty() {
            return Err(RelayError::Unreachable {
                reason: "the other device hasn't been found on this network".into(),
            });
        }
        let peer_id = device_id.clone();
        let id = self
            .rt
            .command("begin-send", move |state| {
                state.change(|r| Ok(r.begin(&peer_id, true, item, now_ms())))
            })
            .await?;
        let me = self.me_now();
        let weak = self.me.clone();
        let work = self.rt.finishing(cancellation.as_deref());
        let expect = device_id.clone();
        let result = self
            .rt
            .run("send", work, move |cancel| {
                let report = |done: u64| {
                    if let Some(core) = weak.upgrade() {
                        lock(&core.progress).insert(id, done);
                    }
                };
                let mut link = Link::dial(&addresses, &me, Purpose::Session, &secret, &expect)?;
                match &payload {
                    Payload::Note(text) => {
                        link.send(Outgoing::Note(text), report, || cancel.is_cancelled())?
                    }
                    Payload::File { source, name, size } => {
                        let sha256 = forge_storage::files::sha256_file(source)?;
                        link.send(
                            Outgoing::File {
                                path: source,
                                name,
                                size: *size,
                                sha256: &sha256,
                            },
                            report,
                            || cancel.is_cancelled(),
                        )?
                    }
                }
                let reached = (link.them.clone(), link.address.clone());
                link.close();
                Ok::<_, RelayError>(reached)
            })
            .await;
        lock(&self.progress).remove(&id);
        let status = match &result {
            Ok(_) => SavedStatus::Done,
            Err(err) => SavedStatus::Failed {
                reason: err.to_string(),
            },
        };
        let reached = result.as_ref().ok().cloned();
        self.rt
            .command("finish-send", move |state| {
                state.change(|r| {
                    r.finish(id, status, None);
                    if let Some((them, address)) = reached {
                        r.reached(&them.device_id, &them.name, Some(address));
                    }
                    Ok(())
                })
            })
            .await?;
        result?;
        self.transfers()
            .into_iter()
            .find(|t| t.id == id)
            .ok_or(RelayError::NotFound { id })
    }
}

impl Drop for RelayCore {
    fn drop(&mut self) {
        self.stop_listening();
    }
}

/// Accepts connections until stopped or the core is gone; each is answered
/// on its own thread.
fn serve(listener: TcpListener, core: Weak<RelayCore>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let core = core.clone();
                let _ = std::thread::Builder::new()
                    .name("relay-answer".into())
                    .spawn(move || {
                        let Some(core) = core.upgrade() else { return };
                        if stream.set_nonblocking(false).is_err() {
                            return;
                        }
                        let me = core.me_now();
                        let inbox = FileStore::new(&core.inbox).with_max_bytes(MAX_FILE_BYTES);
                        let host = Answering { core: &core };
                        if let Err(err) = link::answer(stream, &me, &inbox, &host) {
                            forge_runtime::logging::info!("relay connection ended: {err}");
                        }
                    });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if core.strong_count() == 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                forge_runtime::logging::warn!("relay accept failed: {err}");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

/// The listening side's decisions, made against the core. Commands are
/// queued from the connection's thread and waited for there.
struct Answering<'a> {
    core: &'a RelayCore,
}

impl Answering<'_> {
    fn command<T: Send + 'static>(
        &self,
        name: &'static str,
        edit: impl FnOnce(&mut Record) -> Result<T, RelayError> + Send + 'static,
    ) -> Result<T, RelayError> {
        forge_runtime::block_on(self.core.rt.command(name, move |state| state.change(edit)))
    }
}

impl Inbound for Answering<'_> {
    fn secret(&self, intro: &Intro) -> Option<[u8; 32]> {
        match intro.purpose {
            Purpose::Pair => {
                let pending = *lock(&self.core.invitation);
                pending
                    .filter(|(_, expires)| now_ms() < *expires)
                    .map(|(secret, _)| secret)
            }
            Purpose::Session => self.core.record.get().secret(&intro.from),
        }
    }

    fn introduced(&self, purpose: &Purpose, secret: &[u8; 32], them: &Them, _address: &str) {
        let them = them.clone();
        let result = match purpose {
            Purpose::Pair => {
                // Used once.
                lock(&self.core.invitation).take();
                let secret = *secret;
                self.command("paired", move |r| {
                    r.pair(&them.device_id, &them.name, &secret, None, now_ms());
                    Ok(())
                })
            }
            Purpose::Session => self.command("reached", move |r| {
                r.reached(&them.device_id, &them.name, None);
                Ok(())
            }),
        };
        if let Err(err) = result {
            forge_runtime::logging::warn!("relay couldn't save a peer: {err}");
        }
    }

    fn offered(&self, them: &Them, item: &Offered) -> Result<u64, String> {
        let saved = match item {
            Offered::Note { text } => SavedItem::Note {
                text: model::clean_note(text).map_err(|e| e.to_string())?,
            },
            Offered::File { name, size, .. } => {
                if *size > MAX_FILE_BYTES {
                    return Err(RelayError::FileTooLarge.to_string());
                }
                SavedItem::File {
                    name: name.clone(),
                    size: *size,
                    file: None,
                }
            }
        };
        let peer = them.device_id.clone();
        self.command("begin-receive", move |r| {
            Ok(r.begin(&peer, false, saved, now_ms()))
        })
        .map_err(|e| e.to_string())
    }

    fn progress(&self, id: u64, done: u64) {
        lock(&self.core.progress).insert(id, done);
    }

    fn finished(&self, id: u64, result: Result<Option<String>, String>) {
        lock(&self.core.progress).remove(&id);
        let (status, file) = match result {
            Ok(file) => (SavedStatus::Done, file),
            Err(reason) => (SavedStatus::Failed { reason }, None),
        };
        if let Err(err) = self.command("finish-receive", move |r| {
            r.finish(id, status, file);
            Ok(())
        }) {
            forge_runtime::logging::warn!("relay couldn't save a transfer: {err}");
        }
    }
}

forge_app::export_runtime!(RelayCore, rt);
