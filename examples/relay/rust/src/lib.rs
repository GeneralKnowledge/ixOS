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
//! - **Asking another device's model** (protocol 2, docs/forge-ai-plan.md
//!   A1): `ask` sends Forge AI's request to a paired device and streams the
//!   answer back; `answer_asks` makes this device answer paired devices'
//!   asks with a model, one at a time. The `relay serve-ai` command answers
//!   with any OpenAI-compatible server (llama.cpp, Ollama, LM Studio).
//! - The `relay` command (`--features cli`) is the same code on Linux, so
//!   one iPhone and a laptop are enough to try it.

mod code;
mod code_send;
mod code_session;
pub mod link;
mod model;
mod node;
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
use forge_services::ai::{AiError, ChatRequest, LanguageModel, ModelInfo, TextOut};
use forge_storage::FileStore;

use code::CodeProgress;
pub use code::{CodeReceipt, CODE_SERVICE_TYPE};
use code_send::CodeSendShared;
pub use code_send::{CodeSend, NearbySender};
use node::NodeCell;

pub use link::AskReport;
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
    /// The model this device answers paired devices' asks with.
    answers_with: Mutex<Option<Arc<dyn LanguageModel>>>,
    /// This device on iroh (`node`): its lasting key, endpoint and blob
    /// store, started on first use.
    node: Arc<NodeCell>,
    /// Receiving with a word code (`code`): where files and folders go, how
    /// far it has got, and the stop switch.
    code_dir: PathBuf,
    code_progress: Arc<CodeProgress>,
    code_stop: Mutex<Arc<AtomicBool>>,
    /// Sending with a word code (`code_send`).
    code_send: Arc<CodeSendShared>,
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
        // Absolute, so the history can tell a code receive's path.
        let code_dir = std::path::absolute(directory.path().join("codes"))
            .unwrap_or_else(|_| directory.path().join("codes"));
        // Sends kept their own store before one store served both.
        let _ = std::fs::remove_dir_all(code_dir.join("send-store"));
        let node = Arc::new(NodeCell::new(
            directory.path().to_path_buf(),
            code_dir.join("store"),
        ));
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
            answers_with: Mutex::new(None),
            node,
            code_dir,
            code_progress: Arc::new(CodeProgress::default()),
            code_stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            code_send: Arc::new(CodeSendShared::default()),
            me: me.clone(),
        }))
    }

    /// Receives the file or folder a sender offers with a word code such as
    /// `37-grasp-gag-drum` (Relay M1; the sender is another Relay or
    /// `relay-spike send`). `addresses` and `endpoint_id` come from the
    /// sender's Bonjour advertisement (`CODE_SERVICE_TYPE`, TXT `n` = the
    /// code's number, `id` = the endpoint id). Every chunk is verified, and
    /// it is saved in Relay's `codes/received` folder; a dropped receive
    /// resumes with the sender's next code, also after a relaunch. Once the
    /// sender has named what it sends, the receive goes into the history.
    /// One at a time.
    pub async fn receive_with_code(
        &self,
        code: String,
        addresses: Vec<String>,
        endpoint_id: String,
    ) -> Result<CodeReceipt, RelayError> {
        if self.code_progress.active.swap(true, Ordering::SeqCst) {
            return Err(RelayError::Declined {
                reason: "already receiving".into(),
            });
        }
        let stop = Arc::new(AtomicBool::new(false));
        *lock(&self.code_stop) = Arc::clone(&stop);
        let node = match self.node().await {
            Ok(node) => node,
            Err(err) => {
                self.code_progress.active.store(false, Ordering::SeqCst);
                return Err(err);
            }
        };
        let result = code::receive(
            &code,
            &addresses,
            &endpoint_id,
            node,
            &self.code_dir.join("received"),
            Arc::clone(&self.code_progress),
            stop,
        )
        .await;
        let entry = match &result {
            Ok(r) => Some((
                r.name.clone(),
                r.size,
                Some(r.path.clone()),
                SavedStatus::Done,
            )),
            Err(failed) => failed.offered.as_ref().map(|o| {
                (
                    o.name.clone(),
                    o.size,
                    None,
                    SavedStatus::Failed {
                        reason: failed.error.to_string(),
                    },
                )
            }),
        };
        if let Some((name, size, file, status)) = entry {
            let item = SavedItem::File { name, size, file };
            if let Err(err) = self
                .rt
                .command("code-receive", move |state| {
                    state.change(|r| Ok(r.code_transfer(false, item, status, now_ms())))
                })
                .await
            {
                forge_runtime::logging::warn!("relay couldn't save a code receive: {err}");
            }
        }
        result.map_err(|f| f.error)
    }

    /// Stops the receive in progress, if any; it ends with an error, and
    /// what arrived stays for a resume.
    pub fn cancel_code_receive(&self) {
        lock(&self.code_stop).store(true, Ordering::SeqCst);
    }

    /// The size of the file being received with a code, and how much of it
    /// was already here from an earlier try; `None` when none is running or
    /// the sender hasn't named the file yet.
    pub fn code_receive_size(&self) -> Option<CodeSize> {
        if !self.code_progress.active.load(Ordering::SeqCst) {
            return None;
        }
        let total = self.code_progress.total.load(Ordering::SeqCst);
        (total > 0).then(|| CodeSize {
            total,
            already: self.code_progress.already.load(Ordering::SeqCst),
            done: self.code_progress.done.load(Ordering::SeqCst),
        })
    }

    /// Starts sending the file or folder at `path` with a new word code, in
    /// the background (Relay M1). Follow it with `code_send_status`: while it
    /// has a code, the app advertises it with Bonjour and reports other
    /// senders it sees with `code_send_nearby`. The receiver is another
    /// Relay, or `relay-spike receive <code>` for a file. When it ends it
    /// goes into the history. One at a time.
    pub fn send_with_code(&self, path: String) -> Result<(), RelayError> {
        let path = PathBuf::from(path);
        if !path.is_file() && !path.is_dir() {
            return Err(RelayError::File {
                reason: "there is no file or folder there".into(),
            });
        }
        if self.code_send.running() {
            return Err(RelayError::Declined {
                reason: "already sending".into(),
            });
        }
        let weak = self.me.clone();
        let sent = path.clone();
        let on_end: code_send::OnEnd = Box::new(move |end: &CodeSend| {
            let status = match end {
                CodeSend::Sent { .. } => SavedStatus::Done,
                CodeSend::Failed { reason } => SavedStatus::Failed {
                    reason: reason.clone(),
                },
                _ => return,
            };
            // Off Relay's iroh runtime: saving waits for the command thread.
            std::thread::spawn(move || {
                let Some(core) = weak.upgrade() else { return };
                let name = sent
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into());
                let size = if sent.is_dir() {
                    code_send::files_in(&sent)
                        .map(|f| f.iter().map(|(_, _, n)| n).sum())
                        .unwrap_or(0)
                } else {
                    std::fs::metadata(&sent).map(|m| m.len()).unwrap_or(0)
                };
                let item = SavedItem::File {
                    name,
                    size,
                    file: Some(sent.to_string_lossy().into_owned()),
                };
                let saved = forge_runtime::block_on(core.rt.command("code-send", move |state| {
                    state.change(|r| Ok(r.code_transfer(true, item, status, now_ms())))
                }));
                if let Err(err) = saved {
                    forge_runtime::logging::warn!("relay couldn't save a code send: {err}");
                }
            });
        });
        code_send::start(
            path,
            Arc::clone(&self.node),
            Arc::clone(&self.code_send),
            on_end,
        );
        Ok(())
    }

    /// Where the send with a code has got.
    pub fn code_send_status(&self) -> CodeSend {
        self.code_send.status()
    }

    /// The other senders the app sees advertising nearby, replacing the last
    /// list. A new code avoids their numbers, and if one takes this code's
    /// number before it is used, the code is replaced.
    pub fn code_send_nearby(&self, senders: Vec<NearbySender>) {
        *lock(&self.code_send.nearby) = senders.into_iter().collect();
    }

    /// Stops the send in progress, if any.
    pub fn cancel_code_send(&self) {
        self.code_send.stop.store(true, Ordering::SeqCst);
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
            |name| match self.code_received(name) {
                Some(path) => path.exists().then(|| name.to_string()),
                None => inbox.get(name).map(|p| p.to_string_lossy().into_owned()),
            },
            |id| progress.get(&id).copied().unwrap_or(0),
        )
    }

    /// Removes a history entry and, for a received file, the file.
    pub async fn remove_transfer(&self, id: u64) -> Result<(), RelayError> {
        let received = CodeReceived(self.code_dir.join("received"));
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
                    match received.code_received(&name) {
                        // A file or folder received with a code.
                        Some(path) if path.is_dir() => {
                            let _ = std::fs::remove_dir_all(path);
                        }
                        Some(path) => {
                            let _ = std::fs::remove_file(path);
                        }
                        None => {
                            state.inbox.remove(&name);
                        }
                    }
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

/// The file or folder a code receive is fetching.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CodeSize {
    pub total: u64,
    /// Bytes that were here before this try (a resume).
    pub already: u64,
    /// Bytes here so far, counting `already`.
    pub done: u64,
}

/// Where code receives land (`codes/received`).
struct CodeReceived(PathBuf);

impl CodeReceived {
    /// The received file or folder a history entry names, when it is one
    /// (a code receive saves its full path; the old inbox saves a name).
    fn code_received(&self, name: &str) -> Option<PathBuf> {
        let path = std::path::Path::new(name);
        (path.is_absolute() && path.starts_with(&self.0)).then(|| path.to_path_buf())
    }
}

/// The number a word code starts with (what the sender advertises), or why
/// the code can't be right as typed. The app uses it to find the sender.
#[uniffi::export]
pub fn word_code_number(code: String) -> Result<u8, RelayError> {
    code::WordCode::parse(&code).map(|c| c.number)
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
    /// Answers paired devices' asks with `model`, one at a time (another
    /// ask meanwhile gets `Busy`); `None` stops answering. Used by `relay
    /// serve-ai`; the app gets it with the AI Lab (A2).
    pub fn answer_asks(&self, model: Option<Arc<dyn LanguageModel>>) {
        *lock(&self.answers_with) = model.map(|model| {
            Arc::new(OneAtATime {
                model,
                busy: Mutex::new(()),
            }) as Arc<dyn LanguageModel>
        });
    }

    /// Asks a paired device's model (at `addresses`, or where it was last
    /// reached). The answer arrives in `out` piece by piece; when `out`
    /// says stop, the other device stops its model. Blocks until the answer
    /// ends: call it from a worker thread. The outer result is the
    /// connection's, the inner the model's (`Ok` with the model's name).
    pub fn ask(
        &self,
        device_id: &str,
        request: ChatRequest,
        addresses: Vec<String>,
        out: &dyn TextOut,
    ) -> Result<Result<String, AiError>, RelayError> {
        let (addresses, secret) = self.route(device_id, addresses)?;
        let mut link = Link::dial(
            &addresses,
            &self.me_now(),
            Purpose::Session,
            &secret,
            device_id,
        )?;
        let answer = link.ask(request, out)?;
        let (them, address) = (link.them.clone(), link.address.clone());
        link.close();
        let saved = forge_runtime::block_on(self.rt.command("reached", move |state| {
            state.change(|r| {
                r.reached(&them.device_id, &them.name, Some(address));
                Ok(())
            })
        }));
        if let Err(err) = saved {
            forge_runtime::logging::warn!("relay couldn't save where a peer was: {err}");
        }
        Ok(answer)
    }

    /// Where to reach a paired device, and the pair's secret.
    fn route(
        &self,
        device_id: &str,
        addresses: Vec<String>,
    ) -> Result<(Vec<String>, [u8; 32]), RelayError> {
        let record = self.record.get();
        let peer = record
            .peer(device_id)
            .ok_or_else(|| RelayError::UnknownPeer {
                id: device_id.to_string(),
            })?;
        let secret = record.secret(device_id).ok_or(RelayError::Refused)?;
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
        Ok((addresses, secret))
    }

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
        let (addresses, secret) = self.route(&device_id, addresses)?;
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

impl RelayCore {
    /// This device's iroh node, started on Relay's runtime on first use.
    async fn node(&self) -> Result<Arc<node::Node>, RelayError> {
        let cell = Arc::clone(&self.node);
        code::TOKIO
            .spawn(async move { cell.get().await })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)))
    }

    fn code_received(&self, name: &str) -> Option<PathBuf> {
        CodeReceived(self.code_dir.join("received")).code_received(name)
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

/// A model that answers one ask at a time.
struct OneAtATime {
    model: Arc<dyn LanguageModel>,
    busy: Mutex<()>,
}

impl LanguageModel for OneAtATime {
    fn describe(&self) -> ModelInfo {
        self.model.describe()
    }

    fn generate(&self, request: ChatRequest, out: Arc<dyn TextOut>) -> Result<(), AiError> {
        let Ok(_turn) = self.busy.try_lock() else {
            return Err(AiError::Busy {
                reason: "it is answering another request; try again shortly".into(),
            });
        };
        self.model.generate(request, out)
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

    fn model(&self) -> Option<Arc<dyn LanguageModel>> {
        lock(&self.core.answers_with).clone()
    }

    fn answered(&self, them: &Them, report: &AskReport) {
        // Timings only, never the text.
        forge_runtime::logging::info!(
            "relay answered an ask from {}: model={} pieces={} ms={} error={:?}",
            them.name,
            report.model,
            report.pieces,
            report.elapsed.as_millis(),
            report.error,
        );
    }
}

forge_app::export_runtime!(RelayCore, rt);
