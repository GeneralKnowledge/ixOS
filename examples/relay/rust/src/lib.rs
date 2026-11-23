//! Relay: send notes, files and messages between your devices, encrypted,
//! on iroh.
//!
//! The app's whole Rust side, built on the application runtime
//! (`forge_app`). Everything goes over one iroh endpoint (`node`):
//!
//! - **Word codes** (`code`, `code_send`): anyone sends a file or folder
//!   with a code such as `37-grasp-gag-drum`; the app shows it as words
//!   and as a QR code, and the receiver types or scans it.
//! - **Trusted devices** (`trust`, `device`): after a code transfer the
//!   person can trust the other device (the pairing policy). Trusted
//!   devices send notes, files, conversation messages and AI asks over
//!   `relay/device/1` without a code, found by the resolver (`resolver`).
//! - **Asking another device's model** (docs/forge-ai-plan.md A1): `ask`
//!   sends Forge AI's request to a trusted device and streams the answer
//!   back; `answer_asks` makes this device answer trusted devices' asks
//!   with a model, one at a time. The `relay serve-ai` command answers
//!   with any OpenAI-compatible server (llama.cpp, Ollama, LM Studio).
//! - The `relay` command (`--features cli`) is the same code on Linux, so
//!   one iPhone and a laptop are enough to try it.
//!
//! The first Relay paired devices by QR code over TCP and Noise; that path
//! is gone (M1 PR 3b). Its history stays readable.

mod capability;
pub mod chat;
mod code;
mod code_send;
mod code_session;
mod device;
pub mod device_chat;
mod migrate;
mod model;
mod node;
mod outbox;
mod presence;
mod record;
mod resolver;
mod store;
pub use resolver::{own_relay_servers, ConnectionMode, RelayServers, RouteKind, ServerHealth};
mod hex;
mod mailbox;
mod mailbox_client;
pub mod mailbox_server;
mod trust;
mod web;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use forge_app::{DataDirectory, Published, Runtime};
use forge_runtime::{lock, now_ms};
use forge_services::ai::{AiError, ChatRequest, LanguageModel, ModelInfo, TextOut};
use forge_storage::FileStore;

pub use capability::Capability;
use code::CodeProgress;
pub use code::{CodeReceipt, CODE_SERVICE_TYPE};
use code_send::CodeSendShared;
pub use code_send::{CodeSend, NearbySender};
#[doc(hidden)]
pub use node::set_store_cleanup_interval;
use node::NodeCell;
pub use presence::Presence;
pub use trust::PairingPolicy;
use trust::{AfterCode, SavedTrust};
pub use web::{SiteHeader, SiteRequest, SiteResponse, SiteSource, Tunnel as SitePort};

pub use device::AskReport;
pub use model::{
    relay_error_kind, Direction, Identity, Item, RelayError, Transfer, TransferStatus,
    MAX_FILE_BYTES, MAX_NAME_CHARS, MAX_NOTE_CHARS,
};
use record::{Record, SavedItem, SavedStatus};

uniffi::setup_scaffolding!();

// `forge_log` for the host: Swift's `ForgeLog("Category")` writes to the same
// log as Rust.
forge_runtime::log_export!();

// Native services the host implements (Keychain, directories, notifications,
// permissions, deep links, audio, Face ID, URLSession, the on-device model,
// background refresh), as `crate::native`. The app installs them with
// `ForgeServices.install()`; see docs/native-services.md. Messaging
// (`chat`) uses them.
forge_services::native_services!(audio, biometrics, http, ai, background);

/// What the command thread owns.
struct State {
    /// The record as saved in `store`.
    record: Record,
    store: Arc<store::Store>,
    logs: device_chat::PeerLogs,
    inbox: FileStore,
    published: Arc<Published<Record>>,
}

impl State {
    fn change<T>(
        &mut self,
        edit: impl FnOnce(&mut Record) -> Result<T, RelayError>,
    ) -> Result<T, RelayError> {
        self.change_with(|r, _| edit(r))
    }

    /// Changes the record and device conversations in one transaction:
    /// all of it is saved, or none (`store`).
    fn change_with<T>(
        &mut self,
        edit: impl FnOnce(&mut Record, &mut device_chat::Conversations<'_>) -> Result<T, RelayError>,
    ) -> Result<T, RelayError> {
        let (value, record, changed) = change_record(&self.store, &self.record, edit)?;
        self.record = record;
        self.published.set(self.record.clone());
        self.logs.notify(changed);
        Ok(value)
    }
}

/// `edit` applied to a copy of `record` and saved, with any conversation
/// changes, in one transaction. Returns the changed record, and the
/// messages that changed for the listeners.
type Changed<T> = (T, Record, Vec<(String, chat_core::Message)>);
fn change_record<T>(
    store: &store::Store,
    record: &Record,
    edit: impl FnOnce(&mut Record, &mut device_chat::Conversations<'_>) -> Result<T, RelayError>,
) -> Result<Changed<T>, RelayError> {
    let mut next = record.clone();
    let (value, changed) = store.write(|c| {
        let mut conversations = device_chat::Conversations::new(c);
        let value = edit(&mut next, &mut conversations)?;
        store::save(c, record, &next)?;
        Ok((value, conversations.changed()))
    })?;
    Ok((value, next, changed))
}

/// Called with a trusted device's endpoint id when it can be reached now.
pub(crate) type ChatWaker = Arc<dyn Fn(&str) + Send + Sync>;

/// The app's core. The Swift app opens one per process and shares it.
#[derive(uniffi::Object)]
pub struct RelayCore {
    rt: Runtime<State>,
    record: Arc<Published<Record>>,
    inbox: PathBuf,
    progress: Mutex<HashMap<u64, u64>>,
    /// The model this device answers trusted devices' asks with.
    answers_with: Mutex<Option<Arc<dyn LanguageModel>>>,
    /// This device on iroh (`node`): its lasting key, endpoint and blob
    /// store, started on first use.
    node: Arc<NodeCell>,
    /// Receiving with a word code (`code`): where files and folders go, how
    /// far it has got, and the stop switch.
    code_dir: PathBuf,
    /// Where code receives land: `codes/received` unless the app chose a
    /// folder the Files app shows ([`RelayCore::receive_into`]).
    received_in: Mutex<PathBuf>,
    code_progress: Arc<CodeProgress>,
    code_stop: Mutex<Arc<AtomicBool>>,
    /// Sending with a word code (`code_send`).
    code_send: Arc<CodeSendShared>,
    /// The device a code transfer was just made with, waiting for the
    /// person to say whether to trust it (Ask to Pair).
    /// Devices code transfers were made with that the person hasn't
    /// answered yet, oldest first: a second transfer doesn't replace the
    /// first's offer.
    trust_offers: Arc<Mutex<VecDeque<TrustOffer>>>,
    /// Trusted devices the app sees nearby, with their addresses.
    nearby: Mutex<HashMap<String, Vec<String>>>,
    /// When each trusted device last got past Hello on a connection it
    /// made here (in memory; Hellos, deliveries and status answers are
    /// saved in the record as `last_seen_ms`). For presence (`presence`).
    reached: Mutex<HashMap<String, i64>>,
    /// Whether the node's addresses are watched for network changes.
    watching_network: AtomicBool,
    /// Why each trusted device wasn't reached the last time Relay tried,
    /// in words (the resolver's diagnostics); gone once it is reached.
    problems: Mutex<HashMap<String, String>>,
    /// The stop switch of each send to a trusted device, by history id.
    device_stops: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    /// What waits for trusted devices, and when to try them (`outbox`).
    outbox: outbox::Outbox,
    /// Each trusted device's conversation log (`device_chat`).
    peer_logs: device_chat::PeerLogs,
    /// Wakes a device conversation's outbox when the device is reached:
    /// set by the chat core that holds those conversations.
    chat_waker: Mutex<Option<ChatWaker>>,
    /// The site this device shares now (N6): in memory only, so on the
    /// iPhone it lasts while Relay is open.
    site: Mutex<Option<SiteSource>>,
    /// One `relay/web/1` connection per device whose site this one opens.
    site_connections: Mutex<HashMap<String, Arc<Mutex<web::Kept>>>>,
    me: Weak<RelayCore>,
}

impl RelayCore {
    /// The way to a trusted device's site: the running node, where to dial
    /// it, and the connection kept for it.
    async fn site_route(
        &self,
        endpoint_id: &str,
    ) -> Result<(Arc<node::Node>, iroh::EndpointAddr, Arc<Mutex<web::Kept>>), RelayError> {
        // Trusted, before anything starts; the route is chosen once the
        // network is known (this network's own IPv6 prefix is Local).
        self.trusted_site_device(endpoint_id)?;
        let node = self.node().await?;
        self.watch_network(&node);
        let addr = self.site_addr(endpoint_id)?;
        let cached = Arc::clone(
            lock(&self.site_connections)
                .entry(endpoint_id.to_string())
                .or_default(),
        );
        Ok((node, addr, cached))
    }

    /// Where a trusted device's site may be dialled now, by the route the
    /// person's Connections allow.
    fn site_addr(&self, endpoint_id: &str) -> Result<iroh::EndpointAddr, RelayError> {
        let saved = self.trusted_site_device(endpoint_id)?;
        let Some((addr, _)) = self.dial(&saved) else {
            return Err(RelayError::Unreachable {
                reason: self.problem_of(endpoint_id).unwrap_or_default(),
            });
        };
        Ok(addr)
    }

    /// `site_addr` for `endpoint_id`, to ask before each request.
    fn site_addr_now(&self, endpoint_id: &str) -> web::SiteRoute {
        let me = self.me.clone();
        let id = endpoint_id.to_string();
        Arc::new(move || {
            me.upgrade()
                .ok_or(RelayError::Refused)
                .and_then(|core| core.site_addr(&id))
        })
    }

    /// The saved record of `endpoint_id`, if it is trusted here.
    fn trusted_site_device(&self, endpoint_id: &str) -> Result<trust::SavedDevice, RelayError> {
        self.record
            .get()
            .devices
            .get(endpoint_id)
            .filter(|d| d.trust == SavedTrust::Trusted)
            .cloned()
            .ok_or_else(|| RelayError::Declined {
                reason: "That device is no longer trusted here.".into(),
            })
    }

    /// Closes the kept site connections: to one device, or (`None`) all.
    /// A dial running at the time isn't kept either.
    fn close_site_connections(&self, endpoint_id: Option<&str>) {
        let kept = lock(&self.site_connections);
        for (id, cached) in kept.iter() {
            if endpoint_id.is_none_or(|e| e == id) {
                lock(cached).close(b"closed");
            }
        }
    }

    /// Opens a site on this computer (N6, `relay web`): a port on
    /// 127.0.0.1 (`port` 0 picks one) whose requests for the device's own
    /// name under `localhost` go to the trusted device's site. Each request
    /// checks the device is still trusted here and dials it only by a route
    /// Connections allow now. It stays open until the returned tunnel is
    /// dropped.
    pub fn open_site_port(&self, endpoint_id: &str, port: u16) -> Result<SitePort, RelayError> {
        let route = self.site_addr_now(endpoint_id);
        let host = web::site_host(endpoint_id);
        forge_runtime::block_on(async {
            let (node, _, cached) = self.site_route(endpoint_id).await?;
            code::TOKIO
                .spawn(async move { web::open_tunnel(node, cached, port, host, route).await })
                .await
                .unwrap_or_else(|e| Err(code::connection(e)))
        })
    }
}

#[uniffi::export]
impl RelayCore {
    /// Opens Relay's data in `data_directory` (created if missing), holding
    /// the directory so no second core can write to it. The first open
    /// makes this device's identity, named `device_name` (the person can
    /// rename it).
    ///
    /// Relay's own servers are on unless the person chose otherwise (N2).
    #[uniffi::constructor]
    pub fn new(data_directory: String, device_name: String) -> Result<Arc<Self>, RelayError> {
        Self::with_default_servers(data_directory, device_name, own_relay_servers())
    }

    /// `new` for tests and previews: a device that never chose gets no
    /// servers, so nothing reaches the real ones.
    #[uniffi::constructor]
    pub fn without_default_servers(
        data_directory: String,
        device_name: String,
    ) -> Result<Arc<Self>, RelayError> {
        Self::with_default_servers(data_directory, device_name, RelayServers::default())
    }
}

impl RelayCore {
    /// `new`, with `default_servers` as the servers a device that never
    /// chose gets. Tests pass none, so they never reach the real servers.
    pub fn with_default_servers(
        data_directory: String,
        device_name: String,
        default_servers: RelayServers,
    ) -> Result<Arc<Self>, RelayError> {
        forge_runtime::logging::init();
        let directory = DataDirectory::open(&data_directory)?;
        // One database (P3); the JSON files move into it on the first open.
        let (store, saved, found) = store::Store::open(directory.path())?;
        let store = Arc::new(store);
        // Whether this folder held Relay before: a fresh install hears
        // nothing about the servers being on.
        let existed = saved.is_some();
        let mut record = saved.unwrap_or_default();
        // Each change at open is a transaction of its own.
        let opening = |record: &mut Record,
                       edit: &mut dyn FnMut(
            &mut Record,
            &mut device_chat::Conversations<'_>,
        ) -> Result<(), RelayError>|
         -> Result<(), RelayError> {
            let ((), next, _) = change_record(&store, record, |r, c| edit(r, c))?;
            *record = next;
            Ok(())
        };
        if record.name.is_empty() {
            let name = model::clean_name(&device_name).unwrap_or_else(|_| "Relay".into());
            opening(&mut record, &mut |r, _| {
                r.name = name.clone();
                Ok(())
            })?;
        }
        if record
            .transfers
            .iter()
            .any(|t| t.status == SavedStatus::InProgress)
        {
            opening(&mut record, &mut |r, _| {
                r.interrupted();
                Ok(())
            })?;
        }
        let inbox = FileStore::new(directory.path().join("inbox")).with_max_bytes(MAX_FILE_BYTES);
        inbox.sweep_temporaries();
        let inbox_root = inbox.root().to_path_buf();
        // Absolute, so the history can tell a code receive's path.
        let code_dir = std::path::absolute(directory.path().join("codes"))
            .unwrap_or_else(|_| directory.path().join("codes"));
        // Sends kept their own store before one store served both.
        let _ = std::fs::remove_dir_all(code_dir.join("send-store"));
        let key_dir = directory.path().to_path_buf();
        // What couldn't be read is said, not only logged.
        if let Some(notice) = store::notice(&found) {
            opening(&mut record, &mut |r, _| {
                r.notice = Some(notice.clone());
                Ok(())
            })?;
        }
        // Notes become messages, once (review step 4).
        if !record.notes_are_messages {
            opening(&mut record, &mut |r, c| migrate::run(r, c))?;
        }
        // The endpoint id is the only device id (P4): the random id an
        // older build made is retired, and its own messages are authored
        // by this device's local label instead, in the same transaction.
        if !record.device_id.is_empty() {
            let old = format!("relay:{}", record.device_id);
            opening(&mut record, &mut |r, c| {
                c.reauthor_own(&old)?;
                r.device_id.clear();
                Ok(())
            })?;
        }
        // Away from home (N2): Relay's servers on, unless the person chose
        // otherwise (another mode, or servers of their own). Applied once;
        // an upgrade that turns them on says so.
        if !record.devices.servers_settled {
            let turn_on = record.devices.mode == ConnectionMode::Hybrid
                && record.devices.servers == RelayServers::default()
                && default_servers != RelayServers::default();
            let used_before = existed;
            opening(&mut record, &mut |r, _| {
                if turn_on {
                    r.devices.servers = default_servers.clone();
                    r.away_notice = used_before;
                }
                r.devices.servers_settled = true;
                Ok(())
            })?;
        }
        // A message the conversation shows queued that the outbox doesn't
        // hold (saved by a build before one transaction held both) failed,
        // so the person can send it again.
        opening(&mut record, &mut |r, c| {
            outbox::settle_messages(r, c);
            Ok(())
        })?;
        let published = Arc::new(Published::new(record.clone()));
        let peer_logs =
            device_chat::PeerLogs::new(directory.path().join("devices"), Arc::clone(&store));
        let store_dir = code_dir.join("store");
        // Outside Hybrid the endpoint has no relay and no lookup server.
        let servers = record
            .devices
            .servers
            .clone()
            .upgraded()
            .in_mode(record.devices.mode);
        let state = State {
            record,
            store,
            logs: peer_logs.clone(),
            inbox,
            published: Arc::clone(&published),
        };
        let core = Arc::new_cyclic(|me| Self {
            rt: Runtime::with_directory("relay", directory, state),
            record: published,
            inbox: inbox_root,
            progress: Mutex::new(HashMap::new()),
            answers_with: Mutex::new(None),
            node: Arc::new(NodeCell::new(
                key_dir,
                store_dir,
                Arc::new(CoreDevices { core: me.clone() }),
                servers,
            )),
            received_in: Mutex::new(code_dir.join("received")),
            code_dir,
            code_progress: Arc::new(CodeProgress::default()),
            code_stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            code_send: Arc::new(CodeSendShared::default()),
            trust_offers: Arc::new(Mutex::new(VecDeque::new())),
            nearby: Mutex::new(HashMap::new()),
            reached: Mutex::new(HashMap::new()),
            problems: Mutex::new(HashMap::new()),
            watching_network: AtomicBool::new(false),
            device_stops: Mutex::new(HashMap::new()),
            outbox: outbox::Outbox::default(),
            peer_logs,
            chat_waker: Mutex::new(None),
            site: Mutex::new(None),
            site_connections: Mutex::new(HashMap::new()),
            me: me.clone(),
        });
        Ok(core)
    }
}

#[uniffi::export]
impl RelayCore {
    /// Relay's servers were turned on by an upgrade (N2): what to tell the
    /// person, once, until they dismiss it. `None` normally.
    pub fn away_from_home_notice(&self) -> Option<String> {
        self.record.get().away_notice.then(|| {
            "Relay now reaches your devices away from home through its servers. What your \
             devices send each other stays encrypted end to end. You can change this in \
             Settings, under Connections."
                .to_string()
        })
    }

    /// The person has read the away-from-home notice.
    pub async fn dismiss_away_from_home_notice(&self) -> Result<(), RelayError> {
        self.rt
            .command("dismiss-away-notice", |state| {
                state.change(|r| {
                    r.away_notice = false;
                    Ok(())
                })
            })
            .await
    }

    /// What Relay couldn't read of its saved data when it opened, and
    /// where that is kept, for the app to show until the person dismisses
    /// it. `None` normally.
    pub fn saved_data_notice(&self) -> Option<SavedDataNotice> {
        self.record.get().notice.clone().map(|n| SavedDataNotice {
            message: n.message,
            kept: n.kept,
        })
    }

    /// The person has read the notice.
    pub async fn dismiss_saved_data_notice(&self) -> Result<(), RelayError> {
        self.rt
            .command("dismiss-notice", |state| {
                state.change(|r| {
                    r.notice = None;
                    Ok(())
                })
            })
            .await
    }

    /// Saves what later code receives bring into `folder` (made if missing)
    /// instead of Relay's own `codes/received`. The app passes a folder in
    /// its Documents so received files show in the Files app; what was
    /// received before stays where it is, and stays in the history.
    pub fn receive_into(&self, folder: String) -> Result<(), RelayError> {
        let folder = std::path::absolute(&folder).map_err(node::file_error)?;
        std::fs::create_dir_all(&folder).map_err(node::file_error)?;
        *lock(&self.received_in) = folder;
        Ok(())
    }

    /// Receives the file or folder a sender offers with a word code such as
    /// `37-grasp-gag-drum` (Relay M1; the sender is another Relay or
    /// `relay send`). `addresses` and `endpoint_id` come from the
    /// sender's Bonjour advertisement (`CODE_SERVICE_TYPE`, TXT `n` = the
    /// code's number, `id` = the endpoint id). Every chunk is verified, and
    /// it is saved in the folder [`RelayCore::receive_into`] chose (the app
    /// passes Documents/Relay), or Relay's own `codes/received` if none was
    /// chosen; several items arrive as one folder. A dropped receive
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
        let into = lock(&self.received_in).clone();
        let result = code::receive(
            &code,
            &addresses,
            &endpoint_id,
            node,
            &into,
            self.identity().name,
            Arc::clone(&self.code_progress),
            stop,
        )
        .await;
        if let Ok(receipt) = &result {
            self.after_code(receipt.sender_id.clone(), receipt.sender_name.clone())
                .await;
        }
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
        self.send_paths_with_code(vec![path])
    }

    /// Starts sending several files and folders as one transfer with a new
    /// word code, like `send_with_code`. The receiver gets them in one
    /// folder named "N items"; two items with the same name are refused.
    pub fn send_paths_with_code(&self, paths: Vec<String>) -> Result<(), RelayError> {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        if paths.is_empty() {
            return Err(RelayError::File {
                reason: "nothing to send".into(),
            });
        }
        if let Some(missing) = paths.iter().find(|p| !p.is_file() && !p.is_dir()) {
            return Err(RelayError::File {
                reason: format!("there is no file or folder at {}", missing.display()),
            });
        }
        if self.code_send.running() {
            return Err(RelayError::Declined {
                reason: "already sending".into(),
            });
        }
        *lock(&self.code_send.my_name) = self.identity().name;
        let weak = self.me.clone();
        let sent = paths.clone();
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
                let name = code_send::send_name(&sent);
                let size = code_send::items_in(&sent)
                    .map(|f| f.iter().map(|(_, _, n)| n).sum())
                    .unwrap_or(0);
                // The history links one item; several have no one path.
                let file = match sent.as_slice() {
                    [one] => Some(one.to_string_lossy().into_owned()),
                    _ => None,
                };
                let item = SavedItem::File { name, size, file };
                let saved = forge_runtime::block_on(core.rt.command("code-send", move |state| {
                    state.change(|r| Ok(r.code_transfer(true, item, status, now_ms())))
                }));
                if let Err(err) = saved {
                    forge_runtime::logging::warn!("relay couldn't save a code send: {err}");
                }
                let receiver = lock(&core.code_send.receiver).take();
                if let Some((id, name)) = receiver {
                    forge_runtime::block_on(core.after_code(id, name));
                }
            });
        });
        code_send::start(
            paths,
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
            now_ms(),
        )
    }

    /// Removes a history entry and, for a received file, the file.
    pub async fn remove_transfer(&self, id: u64) -> Result<(), RelayError> {
        // Taken off the outbox first, if it is waiting: it is never sent.
        // One already in the recipient's mailbox still arrives; only its
        // row goes.
        self.cancel_delivery(id).await;
        let received = self.received();
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

/// The folders code receives land in (`codes/received`, and the folder the
/// app chose).
struct CodeReceived(Vec<PathBuf>);

impl CodeReceived {
    /// The received file or folder a history entry names, when it is one
    /// (a code receive saves its full path; the old inbox saves a name).
    fn code_received(&self, name: &str) -> Option<PathBuf> {
        let path = std::path::Path::new(name);
        (path.is_absolute() && self.0.iter().any(|root| path.starts_with(root)))
            .then(|| path.to_path_buf())
    }
}

/// The number a word code starts with (what the sender advertises), or why
/// the code can't be right as typed. The app uses it to find the sender.
#[uniffi::export]
pub fn word_code_number(code: String) -> Result<u8, RelayError> {
    code::WordCode::parse(&code).map(|c| c.number)
}

/// A device trusted here (or revoked: then it is listed only so the person
/// can trust it again).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Device {
    /// Its iroh endpoint id: what trust is keyed by.
    pub endpoint_id: String,
    /// The name it had when trusted.
    pub name: String,
    /// False when revoked.
    pub trusted: bool,
    pub since_ms: i64,
    /// Whether it may send files and notes here.
    pub receives_from: bool,
    /// Whether it may ask this device's AI (off until turned on).
    pub may_ask: bool,
    /// Whether it introduces devices here (My Devices): the devices it
    /// trusts are offered, and the ones it revokes are revoked here.
    pub introduces: bool,
    /// Whether it may see the devices trusted here, to be introduced to
    /// them.
    pub sees_devices: bool,
    /// The name of the introducer that recommended it, if one did.
    pub introduced_by: Option<String>,
    /// The name of the introducer that revoked it, when the person didn't.
    pub revoked_by: Option<String>,
    /// The person it belongs to, when it isn't one of the person's own
    /// devices (N4 step 1). Chats shows this name rather than `name`.
    pub person: Option<String>,
    /// Whether the app sees it on this network now.
    pub nearby: bool,
    /// Nearby, reachable (a trusted connection worked in the last few
    /// minutes) or offline. Always offline for a revoked device.
    pub presence: Presence,
    /// When it was last reached, in ms since the epoch.
    pub last_seen_ms: Option<i64>,
    /// What it last said about itself (`device_status`): its platform, such
    /// as "ios" or "linux", and what it offers. Its own claim, read-only.
    pub platform: Option<String>,
    pub offers: Vec<String>,
    /// The route of the last connection with it, as the app shows it:
    /// "Direct, local network", "Private network", "Direct, internet".
    pub route: Option<String>,
    /// Why Relay couldn't reach it the last time it tried, in words.
    pub problem: Option<String>,
    /// The capabilities granted to it here (N5): none until the person
    /// turns one on for it.
    pub grants: Vec<Capability>,
}

/// Saved data Relay couldn't read when it opened: what happened, in words,
/// and where it is kept on this device.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SavedDataNotice {
    pub message: String,
    pub kept: Vec<String>,
}

/// A device a code transfer was just made with, which the person may
/// trust (the Ask to Pair policy).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TrustOffer {
    pub endpoint_id: String,
    /// What it calls itself: not proof of anything.
    pub name: String,
    /// The name of the trusted device that introduced it (My Devices), when
    /// the offer comes from an introducer rather than a code transfer.
    pub introduced_by: Option<String>,
    /// That introducer's endpoint id.
    pub introducer_id: Option<String>,
}

/// What one introducer's list did here (My Devices, step 1).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DevicesSynced {
    /// Devices it trusts that were offered to the person.
    pub offered: u32,
    /// Devices it revoked that were revoked here.
    pub revoked: u32,
}

/// A trusted device the app found nearby with Bonjour.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NearbyDevice {
    pub endpoint_id: String,
    /// "host:port", from the advertisement.
    pub addresses: Vec<String>,
}

/// What the app advertises so trusted devices can reach this one: the
/// Bonjour service [`CODE_SERVICE_TYPE`] with TXT `d=1` and `id`, on `port`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeviceService {
    pub endpoint_id: String,
    pub port: u16,
    /// Where this device can be reached ("host:port", every interface's
    /// address, Tailscale's too): what it tells trusted devices it meets.
    pub addresses: Vec<String>,
}

/// Trusted devices (brief PR 3): trust after a code transfer, the pairing
/// policy, revoking and forgetting, and notes and files over
/// `relay/device/1` with no code.
#[uniffi::export]
impl RelayCore {
    /// Starts this device on iroh, if it isn't yet, so trusted devices can
    /// reach it. The app advertises the result with Bonjour while Relay is
    /// on screen.
    pub async fn device_service(&self) -> Result<DeviceService, RelayError> {
        let node = self.node().await?;
        self.watch_network(&node);
        self.deliver_waiting();
        self.sync_introducers();
        Ok(DeviceService {
            endpoint_id: node.id().to_string(),
            port: node.port()?,
            addresses: node.addresses(),
        })
    }

    /// Tries every trusted device something waits for, now, and collects
    /// the mailbox: Relay came back to the foreground. (Seeing a device, or
    /// it connecting here, tries it too; so does each mailbox round, every
    /// 30 s while Relay is open, for items the mailbox has let expire.)
    pub fn deliver_waiting(&self) {
        for to in self.record.get().waiting_for() {
            self.try_now(&to);
        }
        self.collect_mailbox();
    }

    /// Trusted and revoked devices, by name. Forgotten ones aren't listed.
    pub fn devices(&self) -> Vec<Device> {
        let nearby = lock(&self.nearby).clone();
        let reached = lock(&self.reached).clone();
        let now = now_ms();
        let record = self.record.get();
        let name_of = |id: &str| {
            record
                .devices
                .get(id)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "a device no longer listed".into())
        };
        let mut list: Vec<Device> = record
            .devices
            .listed()
            .map(|d| {
                let trusted = d.trust == SavedTrust::Trusted;
                let is_nearby = trusted && nearby.contains_key(&d.endpoint_id);
                let last_seen_ms = [d.last_seen_ms, reached.get(&d.endpoint_id).copied()]
                    .into_iter()
                    .flatten()
                    .max();
                Device {
                    endpoint_id: d.endpoint_id.clone(),
                    name: d.name.clone(),
                    trusted,
                    since_ms: d.since_ms,
                    receives_from: d.receives_from,
                    may_ask: d.may_ask,
                    introduces: d.introduces,
                    sees_devices: d.sees_devices,
                    introduced_by: d.introduced_by.as_deref().map(name_of),
                    revoked_by: d.revoked_by.as_deref().map(name_of),
                    person: d.person.clone(),
                    nearby: is_nearby,
                    presence: if trusted {
                        presence::presence(is_nearby, last_seen_ms, now)
                    } else {
                        Presence::Offline
                    },
                    last_seen_ms,
                    platform: d.platform.clone(),
                    offers: d.offers.clone(),
                    route: d.route.map(|r| r.label().to_string()),
                    problem: trusted
                        .then(|| lock(&self.problems).get(&d.endpoint_id).cloned())
                        .flatten(),
                    grants: d.capabilities(),
                }
            })
            .collect();
        list.sort_by_key(|d| (!d.trusted, d.name.to_lowercase()));
        list
    }

    /// Asks a trusted device what it says about itself (its platform and
    /// what it offers), and remembers the answer and when it came. Tried
    /// where the resolver says (`dial`), within the connection mode. Returns
    /// the device as `devices` lists it.
    pub async fn device_status(&self, endpoint_id: String) -> Result<Device, RelayError> {
        let saved = self
            .record
            .get()
            .devices
            .get(&endpoint_id)
            .filter(|d| d.trust == SavedTrust::Trusted)
            .cloned()
            .ok_or_else(|| RelayError::UnknownPeer {
                id: endpoint_id.clone(),
            })?;
        // This device's own networks are known before dialling, so an
        // address on one of them counts as the local network.
        let node = self.node().await?;
        self.watch_network(&node);
        let Some((addr, _)) = self.dial(&saved) else {
            return Err(RelayError::Unreachable {
                reason: self.problem_of(&endpoint_id).unwrap_or_default(),
            });
        };
        let node = Arc::clone(&self.node);
        let my_name = self.identity().name;
        let report = code::TOKIO
            .spawn(async move {
                let node = node.get().await?;
                device::ask_status(&node, addr, my_name).await
            })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)))?;
        if saved.introduces {
            self.sync_later(endpoint_id.clone());
        }
        let id = endpoint_id.clone();
        self.rt
            .command("device-status", move |state| {
                state.change(|r| {
                    r.devices
                        .reported(&id, &report.platform, &report.offers, now_ms());
                    Ok(())
                })
            })
            .await?;
        self.devices()
            .into_iter()
            .find(|d| d.endpoint_id == endpoint_id)
            .ok_or(RelayError::UnknownPeer { id: endpoint_id })
    }

    /// The trusted devices the app sees nearby now, replacing the last list.
    /// A device that appears, or moves, is tried at once if something
    /// waits for it.
    pub fn devices_nearby(&self, devices: Vec<NearbyDevice>) {
        let now: HashMap<String, Vec<String>> = devices
            .into_iter()
            .map(|d| (d.endpoint_id, d.addresses))
            .collect();
        let before = std::mem::replace(&mut *lock(&self.nearby), now.clone());
        for (id, addresses) in &now {
            if before.get(id) != Some(addresses) {
                self.try_now(id);
            }
        }
    }

    /// Which routes trusted devices are reached by.
    pub fn connection_mode(&self) -> ConnectionMode {
        self.record.get().devices.mode
    }

    /// Only Hybrid uses the relay server. Leaving Hybrid lets go of the
    /// relay at once, and any connection that still finds a relay path is
    /// refused; Relay opens without the relay from then on. Coming back to
    /// Hybrid takes the relay up again (fully from the next open, when
    /// Relay opened outside Hybrid).
    pub async fn set_connection_mode(&self, mode: ConnectionMode) -> Result<(), RelayError> {
        self.rt
            .command("connection-mode", move |state| {
                state.change(|r| {
                    r.devices.mode = mode;
                    Ok(())
                })
            })
            .await?;
        self.apply_servers().await;
        self.deliver_waiting();
        Ok(())
    }

    /// Relay's own servers: a relay and a lookup server (M2 PR 7).
    pub fn servers(&self) -> RelayServers {
        self.record.get().devices.servers.clone().upgraded()
    }

    /// Whether Relay's relay answers now: `Off` when none is in use (none
    /// set, or Connections isn't Everywhere), `Connecting` before Relay is
    /// open to devices or just after a change.
    pub fn server_health(&self) -> ServerHealth {
        if self
            .servers()
            .in_mode(self.connection_mode())
            .relay()
            .is_none()
        {
            return ServerHealth::Off;
        }
        match self.node.started() {
            Some(node) => node.relay_health(),
            None => ServerHealth::Connecting,
        }
    }

    /// Sets Relay's servers, used at once.
    pub async fn set_servers(&self, servers: RelayServers) -> Result<(), RelayError> {
        let clean = RelayServers {
            relay_url: servers.relay_url.filter(|u| !u.trim().is_empty()),
            lookup_url: servers.lookup_url.filter(|u| !u.trim().is_empty()),
            mailbox: servers
                .mailbox
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty()),
        };
        clean
            .check()
            .map_err(|reason| RelayError::Unreachable { reason })?;
        self.rt
            .command("servers", move |state| {
                state.change(|r| {
                    r.devices.servers = clean;
                    r.devices.servers_settled = true;
                    Ok(())
                })
            })
            .await?;
        self.apply_servers().await;
        self.deliver_waiting();
        Ok(())
    }

    /// The network changed (another Wi-Fi, Wi-Fi to mobile data): Relay
    /// looks for its routes again and tries what waits.
    pub fn network_changed(&self) {
        if let Some(node) = self.node.started() {
            code::TOKIO.spawn(async move { node.endpoint.network_change().await });
        }
        self.retry_now();
    }

    pub fn pairing_policy(&self) -> PairingPolicy {
        self.record.get().devices.policy
    }

    pub async fn set_pairing_policy(&self, policy: PairingPolicy) -> Result<(), RelayError> {
        self.rt
            .command("pairing-policy", move |state| {
                state.change(|r| {
                    r.devices.policy = policy;
                    Ok(())
                })
            })
            .await
    }

    /// The oldest device a code transfer was with that the pairing policy
    /// asks whether to trust. Ask on both sides; each decides for itself.
    /// Once it is answered, the next one (if any) is offered.
    pub fn trust_offer(&self) -> Option<TrustOffer> {
        lock(&self.trust_offers).front().cloned()
    }

    /// The person said Not Now to the offer `trust_offer` gave.
    pub fn dismiss_trust_offer(&self) {
        lock(&self.trust_offers).pop_front();
    }

    /// The person trusts the device: from the trust offer, or a revoked
    /// device trusted again from the list. The only way a revoked device
    /// becomes trusted again.
    pub async fn trust_device(&self, endpoint_id: String) -> Result<(), RelayError> {
        let offered = {
            let mut offers = lock(&self.trust_offers);
            let at = offers.iter().position(|o| o.endpoint_id == endpoint_id);
            at.and_then(|at| offers.remove(at))
        };
        let introducer = offered.as_ref().and_then(|o| o.introducer_id.clone());
        let name = match offered {
            Some(o) => o.name,
            None => match self.record.get().devices.get(&endpoint_id) {
                Some(d) => d.name.clone(),
                None => {
                    return Err(RelayError::UnknownPeer { id: endpoint_id });
                }
            },
        };
        let name = if name.trim().is_empty() {
            "Unnamed device".to_string()
        } else {
            name
        };
        let offers = self.trust_offers.clone();
        self.rt
            .command("trust-device", move |state| {
                state.change(|r| {
                    match &introducer {
                        Some(by) => r
                            .devices
                            .trust_introduced(&endpoint_id, &name, by, now_ms()),
                        None => r.devices.trust(&endpoint_id, &name, now_ms()),
                    }
                    Ok(())
                })?;
                // A sync that ran after the offer was taken above, but
                // before this command, may have offered the device again.
                lock(&offers).retain(|o| o.endpoint_id != endpoint_id);
                Ok(())
            })
            .await
    }

    /// Ends trust in the device: its trusted connections are refused, and
    /// no policy or code makes it trusted again by itself. One-off code
    /// transfers still work, as with any device.
    pub async fn revoke_device(&self, endpoint_id: String) -> Result<(), RelayError> {
        lock(&self.nearby).remove(&endpoint_id);
        lock(&self.reached).remove(&endpoint_id);
        let endpoint = endpoint_id.clone();
        self.rt
            .command("revoke-device", move |state| {
                state.change(|r| {
                    if r.devices.revoke(&endpoint) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint })
                    }
                })
            })
            .await?;
        // Its site, if this device had it open, closes at once.
        self.close_site_connections(Some(&endpoint_id));
        // What waited for it ends: No longer trusted.
        self.wake(&endpoint_id);
        // Its pass is taken back from the mailbox now.
        self.collect_mailbox();
        Ok(())
    }

    /// Takes the device off the list. Pair Automatically may trust it
    /// again after a code transfer; a revoked device stays revoked.
    pub async fn forget_device(&self, endpoint_id: String) -> Result<(), RelayError> {
        let endpoint = endpoint_id.clone();
        self.rt
            .command("forget-device", move |state| {
                state.change(|r| {
                    if r.devices.forget(&endpoint) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint })
                    }
                })
            })
            .await?;
        self.close_site_connections(Some(&endpoint_id));
        self.wake(&endpoint_id);
        self.collect_mailbox();
        Ok(())
    }

    /// Whether a trusted device may send files and notes here.
    pub async fn set_receives_from(&self, endpoint_id: String, on: bool) -> Result<(), RelayError> {
        self.rt
            .command("receives-from", move |state| {
                state.change(|r| {
                    if r.devices.set_receives_from(&endpoint_id, on) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint_id })
                    }
                })
            })
            .await
    }

    /// Whether a trusted device may ask this device's AI. Off when a
    /// device is trusted; trust never turns it on.
    /// Whether `endpoint_id` introduces devices here (My Devices, step 1):
    /// the devices it trusts are offered to the person, and the ones it
    /// revokes are revoked here, marked with its name. Turning it on asks
    /// for its list at once when it can be reached.
    pub async fn set_introduces(&self, endpoint_id: String, on: bool) -> Result<(), RelayError> {
        let id = endpoint_id.clone();
        self.rt
            .command("introduces", move |state| {
                state.change(|r| {
                    if r.devices.set_introduces(&id, on) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id })
                    }
                })
            })
            .await?;
        if on {
            self.sync_later(endpoint_id);
        }
        Ok(())
    }

    /// Marks a trusted device as a person's, with their name, or (with
    /// `None`) as one of the person's own devices (N4 step 1). A person's
    /// device may send messages and files, never introduces devices or
    /// sees the device list, and Chats shows the person's name.
    pub async fn set_person(
        &self,
        endpoint_id: String,
        person: Option<String>,
    ) -> Result<(), RelayError> {
        let person = person.map(|n| model::clean_name(&n)).transpose()?;
        self.rt
            .command("person", move |state| {
                state.change(|r| {
                    if r.devices.set_person(&endpoint_id, person.as_deref()) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint_id })
                    }
                })
            })
            .await
    }

    /// Whether `endpoint_id` may see the devices trusted here, so this
    /// device can introduce them to it. Off until the person turns it on.
    pub async fn set_sees_devices(&self, endpoint_id: String, on: bool) -> Result<(), RelayError> {
        self.rt
            .command("sees-devices", move |state| {
                state.change(|r| {
                    if r.devices.set_sees_devices(&endpoint_id, on) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint_id })
                    }
                })
            })
            .await
    }

    /// Asks `endpoint_id`, a device that introduces devices here, which
    /// devices it trusts and has revoked, and applies the answer: new ones
    /// are offered (the person decides), revoked ones are revoked here.
    pub async fn sync_devices(&self, endpoint_id: String) -> Result<DevicesSynced, RelayError> {
        let saved = self
            .record
            .get()
            .devices
            .get(&endpoint_id)
            .filter(|d| d.trust == SavedTrust::Trusted)
            .cloned()
            .ok_or_else(|| RelayError::UnknownPeer {
                id: endpoint_id.clone(),
            })?;
        if !saved.introduces {
            return Err(RelayError::Declined {
                reason: format!("{} isn't set to introduce devices here", saved.name),
            });
        }
        // This device's own networks are known before dialling, so an
        // address on one of them counts as the local network.
        let node = self.node().await?;
        self.watch_network(&node);
        let Some((addr, _)) = self.dial(&saved) else {
            return Err(RelayError::Unreachable {
                reason: self.problem_of(&endpoint_id).unwrap_or_default(),
            });
        };
        let node = Arc::clone(&self.node);
        let my_name = self.identity().name;
        let (list, me) = code::TOKIO
            .spawn(async move {
                let node = node.get().await?;
                let me = node.id().to_string();
                device::ask_devices(&node, addr, my_name)
                    .await
                    .map(|list| (list, me))
            })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)))?;
        let offers = self.trust_offers.clone();
        let introducer = endpoint_id.clone();
        let done = self
            .rt
            .command("sync-devices", move |state| {
                let done = state.change(|r| Ok(r.devices.introduced(&introducer, &me, &list)))?;
                // Set within the command, so the change the app hears
                // about already carries the offers.
                let mut waiting = lock(&offers);
                for (id, name) in &done.offers {
                    offer(
                        &mut waiting,
                        TrustOffer {
                            endpoint_id: id.clone(),
                            name: name.clone(),
                            introduced_by: Some(saved.name.clone()),
                            introducer_id: Some(introducer.clone()),
                        },
                    );
                }
                Ok::<_, RelayError>(done)
            })
            .await?;
        for id in &done.revoked {
            lock(&self.nearby).remove(id);
            lock(&self.reached).remove(id);
            // Its site, if this device had it open, closes at once.
            self.close_site_connections(Some(id));
            // What waited for it ends: No longer trusted.
            self.wake(id);
        }
        Ok(DevicesSynced {
            offered: done.offers.len() as u32,
            revoked: done.revoked.len() as u32,
        })
    }

    pub async fn set_may_ask(&self, endpoint_id: String, on: bool) -> Result<(), RelayError> {
        self.rt
            .command("may-ask", move |state| {
                state.change(|r| {
                    if r.devices.set_may_ask(&endpoint_id, on) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint_id })
                    }
                })
            })
            .await
    }

    /// Grants `capability` to a trusted device, or takes it away (N5).
    /// Taking it away ends any use of it at once.
    pub async fn set_grant(
        &self,
        endpoint_id: String,
        capability: Capability,
        on: bool,
    ) -> Result<(), RelayError> {
        self.rt
            .command("grant", move |state| {
                state.change(|r| {
                    if r.devices.set_grant(&endpoint_id, capability, on) {
                        Ok(())
                    } else {
                        Err(RelayError::UnknownPeer { id: endpoint_id })
                    }
                })
            })
            .await
    }

    /// Shares `source` as this device's site (N6), replacing any other:
    /// trusted devices granted `Capability::Sites` can open it while
    /// Relay runs. A folder must exist; a server must be on this device's
    /// localhost.
    pub async fn share_site(&self, source: SiteSource) -> Result<(), RelayError> {
        let source = source.checked()?;
        // The node must be running for anyone to reach the site.
        self.node().await?;
        *lock(&self.site) = Some(source);
        Ok(())
    }

    /// Stops sharing the site: open visits end at once.
    pub fn stop_site(&self) {
        *lock(&self.site) = None;
    }

    /// The site this device shares now, if any.
    pub fn shared_site(&self) -> Option<SiteSource> {
        lock(&self.site).clone()
    }

    /// Asks a trusted device's site for one request (N6), for the iPhone's
    /// in-app viewer: refused unless that device shares a site and has
    /// granted this one `Capability::Sites`.
    pub async fn fetch_site(
        &self,
        endpoint_id: String,
        request: SiteRequest,
    ) -> Result<SiteResponse, RelayError> {
        let (node, _, cached) = self.site_route(&endpoint_id).await?;
        let route = self.site_addr_now(&endpoint_id);
        code::TOKIO
            .spawn(async move { web::fetch(&node, &cached, request, route.as_ref()).await })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)))
    }

    /// Sends files and folders to a trusted device as one transfer, with
    /// no code. Several items arrive in one folder named "N items". They
    /// are copied into Relay's store first, so they can wait in the outbox
    /// (and the originals can go); as for a note, the transfer comes back
    /// Done, or Waiting.
    pub async fn send_to_device(
        &self,
        endpoint_id: String,
        paths: Vec<String>,
    ) -> Result<Transfer, RelayError> {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        if paths.is_empty() {
            return Err(RelayError::File {
                reason: "nothing to send".into(),
            });
        }
        if let Some(missing) = paths.iter().find(|p| !p.is_file() && !p.is_dir()) {
            return Err(RelayError::File {
                reason: format!("there is no file or folder at {}", missing.display()),
            });
        }
        let name = code_send::send_name(&paths);
        let size = code_send::items_in(&paths)
            .map(|f| f.iter().map(|(_, _, n)| n).sum())
            .unwrap_or(0);
        let file = match paths.as_slice() {
            [one] => Some(one.to_string_lossy().into_owned()),
            _ => None,
        };
        let item = SavedItem::File {
            name: name.clone(),
            size,
            file: file.clone(),
        };
        self.trusted(&endpoint_id)?;
        let id = outbox::new_id();
        let node = self.node().await?;
        let offer = code::TOKIO
            .spawn(async move {
                let (offer, _tags) = code_send::prepare(&paths, name, &node, true).await?;
                let content = if offer.collection {
                    iroh_blobs::HashAndFormat::hash_seq(offer.hash)
                } else {
                    iroh_blobs::HashAndFormat::raw(offer.hash)
                };
                node.store
                    .tags()
                    .set(outbox::tag_name(&hex::hex(&id)), content)
                    .await
                    .map_err(node::file_error)?;
                Ok::<_, RelayError>(offer)
            })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)))?;
        // The conversation shows it as a message with one attachment,
        // Queued until the delivery ends.
        let delivery = hex::hex(&id);
        let local = file.as_deref().map(Path::new);
        let attachment = device_chat::files_attachment(
            &hex::hex(offer.hash.as_bytes()),
            offer.collection,
            &offer.name,
            size,
        );
        let me = self.identity();
        let log = self.peer_logs.get(&endpoint_id)?;
        let kept = local.and_then(|file| log.keep(file, &attachment.id));
        let message = chat_core::Message {
            id: device_chat::files_message_id(&delivery),
            author: me.name,
            author_id: device_chat::ME.into(),
            attachments: vec![attachment],
            status: chat_core::MessageStatus::Queued,
            ..chat_core::Message::default()
        };
        // The message and its delivery are saved together (P3).
        let path = kept.or_else(|| local.map(Path::to_path_buf));
        self.queue_with(
            id,
            endpoint_id,
            item,
            outbox::Payload::files(&offer),
            Some((message, path)),
        )
        .await
    }

    /// Stops a send to a trusted device that is still going, or takes it
    /// off the outbox if it is waiting. Does nothing to one already in the
    /// recipient's mailbox ([`TransferStatus::InMailbox`]): it arrives
    /// anyway, so it can't be cancelled.
    pub fn cancel_transfer(&self, id: u64) {
        if self.out_of_hands(id) {
            return;
        }
        if let Some(stop) = lock(&self.device_stops).get(&id) {
            stop.store(true, Ordering::SeqCst);
            return;
        }
        let me = self.me.clone();
        code::TOKIO.spawn(async move {
            if let Some(core) = me.upgrade() {
                core.cancel_delivery(id).await;
            }
        });
    }
}

/// For the command line, where several terminals can answer one offer.
impl RelayCore {
    /// Not Now to the offer for `endpoint_id` only, if it still waits:
    /// another terminal may have answered it already.
    pub fn dismiss_trust_offer_for(&self, endpoint_id: &str) {
        let mut offers = lock(&self.trust_offers);
        if let Some(at) = offers.iter().position(|o| o.endpoint_id == endpoint_id) {
            offers.remove(at);
        }
    }
}

/// What a device on an older build does to send a note (a `DeviceMsg::Note`
/// through the outbox), for the tests of how this build receives one. Not
/// for the app: text to a trusted device is a message in its conversation.
impl RelayCore {
    #[doc(hidden)]
    pub async fn send_note_as_older_build(
        &self,
        endpoint_id: String,
        text: String,
    ) -> Result<Transfer, RelayError> {
        let text = model::clean_note(&text)?;
        self.trusted(&endpoint_id)?;
        let item = SavedItem::Note { text: text.clone() };
        let payload = outbox::Payload::Note { text };
        self.queue(outbox::new_id(), endpoint_id, item, payload)
            .await
    }
}

impl RelayCore {
    /// Answers trusted devices' asks with `model`, one at a time (another
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

    /// Asks a trusted device's model, found by the resolver like any send.
    /// The answer arrives in `out` piece by piece; when `out` says stop,
    /// the other device stops its model. Blocks until the answer ends:
    /// call it from a worker thread, never from async code. The outer
    /// result is the connection's, the inner the model's (`Ok` with the
    /// model's name).
    pub fn ask(
        &self,
        endpoint_id: &str,
        request: ChatRequest,
        out: &dyn TextOut,
    ) -> Result<Result<String, AiError>, RelayError> {
        self.trusted(endpoint_id)?;
        let saved = self
            .record
            .get()
            .devices
            .get(endpoint_id)
            .cloned()
            .ok_or_else(|| RelayError::UnknownPeer {
                id: endpoint_id.to_string(),
            })?;
        let Some((addr, _)) = self.dial(&saved) else {
            return Err(RelayError::Unreachable {
                reason: self.problem_of(endpoint_id).unwrap_or_default(),
            });
        };
        let node = Arc::clone(&self.node);
        let my_name = self.identity().name;
        let (pieces, arriving) = std::sync::mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));
        let asking = code::TOKIO.spawn({
            let stop = Arc::clone(&stop);
            async move {
                let node = node.get().await?;
                device::ask(&node, addr, my_name, request, pieces, stop).await
            }
        });
        // The text goes to `out` here, on the caller's thread, until the
        // other side ends (and drops its end of the channel).
        loop {
            if out.is_stopped() {
                stop.store(true, Ordering::SeqCst);
            }
            match arriving.recv_timeout(Duration::from_millis(100)) {
                Ok(text) => {
                    if !stop.load(Ordering::SeqCst) && !out.push(&text) {
                        stop.store(true, Ordering::SeqCst);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let answer =
            forge_runtime::block_on(asking).unwrap_or_else(|e| Err(code::connection(e)))?;
        lock(&self.problems).remove(endpoint_id);
        lock(&self.reached).insert(endpoint_id.to_string(), now_ms());
        Ok(answer)
    }
}

/// The most offers kept waiting; past it, the oldest goes.
const MOST_OFFERS: usize = 8;

/// Adds an offer behind the others. A device already waiting keeps its
/// place, with the name it gave this time.
fn offer(offers: &mut VecDeque<TrustOffer>, new: TrustOffer) {
    match offers.iter_mut().find(|o| o.endpoint_id == new.endpoint_id) {
        Some(o) => *o = new,
        None => {
            offers.push_back(new);
            if offers.len() > MOST_OFFERS {
                offers.pop_front();
            }
        }
    }
}

impl RelayCore {
    /// Applies the pairing policy after a code transfer with `endpoint_id`,
    /// which called itself `name`.
    async fn after_code(&self, endpoint_id: String, name: String) {
        if endpoint_id.is_empty() {
            return;
        }
        let offers = self.trust_offers.clone();
        let decided: Result<AfterCode, RelayError> = self
            .rt
            .command("after-code", move |state| {
                let decided =
                    state.change(|r| Ok(r.devices.after_code(&endpoint_id, &name, now_ms())))?;
                // Set within the command, so the change the app hears
                // about already carries the offer.
                if decided == AfterCode::Offer {
                    offer(
                        &mut lock(&offers),
                        TrustOffer {
                            endpoint_id,
                            name,
                            introduced_by: None,
                            introducer_id: None,
                        },
                    );
                }
                Ok(decided)
            })
            .await;
        if let Err(err) = decided {
            forge_runtime::logging::warn!("relay couldn't apply the pairing policy: {err}")
        }
    }

    /// The endpoint uses the servers the current mode allows, from now on.
    async fn apply_servers(&self) {
        // Connections to sites were made on the routes the old choice
        // allowed: the next request dials again under the new one.
        self.close_site_connections(None);
        let node = Arc::clone(&self.node);
        let me = self.me.clone();
        // The servers are read once the node's gate is held, so the latest
        // saved choice is the one applied, whatever order changes land in.
        let _ = code::TOKIO
            .spawn(async move {
                node.use_servers(move || {
                    me.upgrade()
                        .map(|core| core.servers().in_mode(core.connection_mode()))
                })
                .await
            })
            .await;
        // A new mailbox gets this device's passes registered now.
        self.collect_mailbox();
    }

    /// Asks the introducer `endpoint_id` for its list in the background;
    /// a failure is only logged (the next open, or Ask Again, tries again).
    fn sync_later(&self, endpoint_id: String) {
        let me = self.me.clone();
        code::TOKIO.spawn(async move {
            let Some(core) = me.upgrade() else { return };
            if let Err(err) = core.sync_devices(endpoint_id).await {
                forge_runtime::logging::info!("relay: an introducer's list wasn't read: {err}");
            }
        });
    }

    /// Asks every device that introduces devices here for its list.
    fn sync_introducers(&self) {
        let introducers: Vec<String> = self
            .record
            .get()
            .devices
            .listed()
            .filter(|d| d.trust == SavedTrust::Trusted && d.introduces)
            .map(|d| d.endpoint_id.clone())
            .collect();
        for id in introducers {
            self.sync_later(id);
        }
    }

    /// Errs unless `endpoint_id` is a trusted device.
    fn trusted(&self, endpoint_id: &str) -> Result<(), RelayError> {
        match self.record.get().devices.get(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted => Ok(()),
            _ => Err(RelayError::UnknownPeer {
                id: endpoint_id.to_string(),
            }),
        }
    }
}

/// The listening side of `relay/device/1`, answered from the core. Called
/// on blocking threads, so commands are waited for directly.
struct CoreDevices {
    core: Weak<RelayCore>,
}

impl CoreDevices {
    fn command<T: Send + 'static>(
        &self,
        name: &'static str,
        edit: impl FnOnce(&mut Record) -> Result<T, RelayError> + Send + 'static,
    ) -> Result<T, RelayError> {
        let core = self.core.upgrade().ok_or(RelayError::Stopped {
            kind: forge_app::ErrorKind::ShutDown,
        })?;
        forge_runtime::block_on(core.rt.command(name, move |state| state.change(edit)))
    }

    /// [`command`](Self::command), changing conversations in the same
    /// transaction.
    fn command_with<T: Send + 'static>(
        &self,
        name: &'static str,
        edit: impl FnOnce(&mut Record, &mut device_chat::Conversations<'_>) -> Result<T, RelayError>
            + Send
            + 'static,
    ) -> Result<T, RelayError> {
        let core = self.core.upgrade().ok_or(RelayError::Stopped {
            kind: forge_app::ErrorKind::ShutDown,
        })?;
        forge_runtime::block_on(core.rt.command(name, move |state| state.change_with(edit)))
    }
}

impl CoreDevices {
    /// A message from `from`, checked, as its conversation keeps it.
    fn peer_message(
        &self,
        from: &str,
        message: device_chat::PeerMessage,
    ) -> Result<chat_core::Message, RelayError> {
        let core = self.core.upgrade().ok_or(RelayError::Stopped {
            kind: forge_app::ErrorKind::ShutDown,
        })?;
        if message.id.is_empty() || message.id.chars().count() > device_chat::MAX_ID_CHARS {
            return Err(RelayError::Connection {
                reason: "a message id that isn't usable".into(),
            });
        }
        let body = model::clean_note(&message.body)?;
        let author = core
            .record
            .get()
            .devices
            .get(from)
            .map(|d| d.shown_name().to_string())
            .unwrap_or_default();
        Ok(chat_core::Message {
            id: message.id,
            author,
            author_id: from.to_string(),
            body,
            reply_to: message.reply_to,
            ..chat_core::Message::default()
        })
    }

    /// The message for what delivery `delivery` saved at `path`, for
    /// `from`'s conversation (added once, by its id).
    fn files_arrived(
        &self,
        from: &str,
        delivery: [u8; 16],
        content: [u8; 32],
        path: &Path,
    ) -> Result<chat_core::Message, RelayError> {
        let core = self.core.upgrade().ok_or(RelayError::Stopped {
            kind: forge_app::ErrorKind::ShutDown,
        })?;
        let id = device_chat::files_message_id(&hex::hex(&delivery));
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = code_send::items_in(&[path.to_path_buf()])
            .map(|f| f.iter().map(|(_, _, n)| n).sum())
            .unwrap_or(0);
        let attachment =
            device_chat::files_attachment(&hex::hex(&content), path.is_dir(), &name, size);
        let author = core
            .record
            .get()
            .devices
            .get(from)
            .map(|d| d.shown_name().to_string())
            .unwrap_or_default();
        Ok(chat_core::Message {
            id,
            author,
            author_id: from.to_string(),
            attachments: vec![attachment],
            ..chat_core::Message::default()
        })
    }
}

impl CoreDevices {
    /// `DeviceInbound::finish_file`, saying whether it was saved: the
    /// mailbox acknowledges an item only once it is.
    pub(crate) fn file_done(
        &self,
        id: u64,
        from: &str,
        delivery: [u8; 16],
        content: [u8; 32],
        result: Result<String, String>,
    ) -> Result<(), RelayError> {
        if let Some(core) = self.core.upgrade() {
            lock(&core.progress).remove(&id);
        }
        // The conversation shows it as a message with the saved file.
        let message = match &result {
            Ok(path) => match self.files_arrived(from, delivery, content, Path::new(path)) {
                Ok(message) => Some((message, PathBuf::from(path))),
                Err(err) => {
                    forge_runtime::logging::warn!("relay couldn't make a files message: {err}");
                    None
                }
            },
            Err(_) => None,
        };
        let (status, file) = match result {
            Ok(path) => (SavedStatus::Done, Some(path)),
            Err(reason) => (SavedStatus::Failed { reason }, None),
        };
        let from = from.to_string();
        // Its message, its id and its history entry: one transaction (P3).
        self.command_with("device-file-done", move |r, c| {
            if let Some((message, path)) = message {
                c.add_files(&from, message, Some(&path))?;
            }
            if status == SavedStatus::Done {
                r.remember_received(&from, &delivery, now_ms());
            }
            r.finish(id, status, file);
            Ok(())
        })
    }
}

impl device::DeviceInbound for CoreDevices {
    fn message(&self, from: &str, message: device_chat::PeerMessage) -> Result<(), RelayError> {
        let core = self.core.upgrade().ok_or(RelayError::Stopped {
            kind: forge_app::ErrorKind::ShutDown,
        })?;
        let message = self.peer_message(from, message)?;
        // One statement, so its own transaction: a message is kept once by
        // its id, which is what makes a retry harmless.
        core.peer_logs.get(from)?.add(message)?;
        Ok(())
    }

    fn granted(&self, from: &str, capability: Capability) -> bool {
        self.core
            .upgrade()
            .is_some_and(|core| core.record.get().devices.granted(from, capability))
    }

    fn site(&self) -> Option<SiteSource> {
        self.core
            .upgrade()
            .and_then(|core| lock(&core.site).clone())
    }

    fn rights(&self, from: &str) -> Option<device::Rights> {
        let core = self.core.upgrade()?;
        let record = core.record.get();
        let saved = record.devices.get(from)?;
        let rights = trust::may_connect(Some(saved.trust)).then_some(device::Rights {
            receives_from: saved.receives_from,
            may_ask: saved.may_ask,
            sees_devices: saved.sees_devices,
        })?;
        // It is here: anything waiting for it goes now.
        core.try_now(from);
        Some(rights)
    }

    fn still_takes_from(&self, from: &str) -> bool {
        self.core.upgrade().is_some_and(|core| {
            core.record
                .get()
                .devices
                .get(from)
                .is_some_and(|d| trust::may_connect(Some(d.trust)) && d.receives_from)
        })
    }

    fn received_before(&self, from: &str, id: &[u8; 16]) -> bool {
        self.core
            .upgrade()
            .is_some_and(|c| c.record.get().received_before(from, id))
    }

    fn my_name(&self) -> String {
        self.core
            .upgrade()
            .map(|c| c.identity().name)
            .unwrap_or_default()
    }

    fn model(&self) -> Option<Arc<dyn LanguageModel>> {
        self.core
            .upgrade()
            .and_then(|core| lock(&core.answers_with).clone())
    }

    fn answered(&self, from: &str, report: &AskReport) {
        // Timings only, never the text.
        // cut: the start of a device id, for the log
        let short: String = from.chars().take(8).collect();
        forge_runtime::logging::info!(
            "relay answered an ask from {short}: model={} pieces={} ms={} error={:?}",
            report.model,
            report.pieces,
            report.elapsed.as_millis(),
            report.error,
        );
    }

    fn device_list(&self, asker: &str) -> trust::DeviceList {
        self.core
            .upgrade()
            .map(|core| core.record.get().devices.list_for(asker))
            .unwrap_or_default()
    }

    fn status(&self, asker: &str) -> device::StatusReport {
        let Some(core) = self.core.upgrade() else {
            return device::StatusReport::default();
        };
        let mut offers: Vec<String> = ["notes", "files", "messages"]
            .into_iter()
            .map(String::from)
            .collect();
        if lock(&core.answers_with).is_some() {
            offers.push("ai".into());
        }
        // A capability is offered only to a device that may use it (N5).
        if lock(&core.site).is_some() && core.record.get().devices.granted(asker, Capability::Sites)
        {
            offers.push(Capability::Sites.name().into());
        }
        device::StatusReport {
            name: core.identity().name,
            platform: std::env::consts::OS.to_string(),
            offers,
        }
    }

    fn reached(&self, from: &str) {
        if let Some(core) = self.core.upgrade() {
            lock(&core.reached).insert(from.to_string(), now_ms());
        }
    }

    fn met(
        &self,
        from: &str,
        addresses: Vec<String>,
        route: Option<RouteKind>,
        mailbox: Option<mailbox::PeerMailbox>,
    ) {
        let Some(core) = self.core.upgrade() else {
            return;
        };
        lock(&core.problems).remove(from);
        // It has just been told this device's pass: registered now, so
        // whatever it leaves while this device is closed is taken.
        core.collect_mailbox();
        let from = from.to_string();
        let now = now_ms();
        code::TOKIO.spawn(async move {
            let _ = core
                .rt
                .command("device-met", move |state| {
                    state.change(|r| {
                        r.devices.met(&from, &addresses, route, mailbox, now);
                        Ok(())
                    })
                })
                .await;
        });
    }

    fn folder(&self) -> PathBuf {
        self.core
            .upgrade()
            .map(|c| lock(&c.received_in).clone())
            .unwrap_or_default()
    }

    /// A note from a device on an older build: a message in its
    /// conversation, `note-<delivery id>`, so a retry adds it once.
    fn note(&self, from: &str, delivery: [u8; 16], text: String) -> Result<(), RelayError> {
        let message = self.peer_message(
            from,
            device_chat::PeerMessage {
                id: format!("note-{}", hex::hex(&delivery)),
                body: text,
                reply_to: None,
            },
        )?;
        let from = from.to_string();
        // The message and the id it came with: one transaction (P3).
        self.command_with("device-note", move |r, c| {
            c.add(&from, message)?;
            r.remember_received(&from, &delivery, now_ms());
            Ok(())
        })
    }

    fn begin_file(&self, from: &str, name: &str, size: u64) -> Result<u64, RelayError> {
        let (from, name) = (from.to_string(), name.to_string());
        self.command("device-file", move |r| {
            Ok(r.begin(
                &from,
                false,
                SavedItem::File {
                    name,
                    size,
                    file: None,
                },
                now_ms(),
            ))
        })
    }

    fn progress(&self, id: u64, done: u64) {
        if let Some(core) = self.core.upgrade() {
            lock(&core.progress).insert(id, done);
        }
    }

    fn finish_file(
        &self,
        id: u64,
        from: &str,
        delivery: [u8; 16],
        content: [u8; 32],
        result: Result<String, String>,
    ) {
        if let Err(err) = self.file_done(id, from, delivery, content, result) {
            forge_runtime::logging::warn!("relay couldn't save a transfer: {err}");
        }
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
        self.received().code_received(name)
    }

    /// The folders code receives landed in: the default and the chosen one.
    fn received(&self) -> CodeReceived {
        CodeReceived(vec![
            self.code_dir.join("received"),
            lock(&self.received_in).clone(),
        ])
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

forge_app::export_runtime!(RelayCore, rt);
