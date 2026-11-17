//! This device on iroh: one endpoint with a key that lasts, and one blob
//! store on disk (Relay M1, [relay-m1-m2-brief.md], "One transfer path").
//!
//! - **The key** is this device's identity. It is made once, saved as
//!   `device.key` in Relay's data directory (0600), and reused on every
//!   launch, so the endpoint id never changes under a connection already
//!   made. PR 3 moves it into the Keychain on iPhone, unchanged.
//! - **One endpoint** serves every protocol on its own ALPN: word codes
//!   (`code`, `code_send`) and iroh-blobs. Relays are off: the local
//!   network only, until M2.
//! - **One store** (`codes/store`) holds what was sent and received, and
//!   survives a relaunch, so an interrupted transfer resumes from what is
//!   already here.
//! - **The gate.** Blobs go only to an endpoint that proved the current
//!   send's code, and only the blobs that send offers.
//!
//! [relay-m1-m2-brief.md]: ../../docs/relay-m1-m2-brief.md

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use iroh::endpoint::{presets, Connection};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointId, RelayMode, SecretKey};
use iroh_blobs::provider::events::{
    AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage, RequestMode,
};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{BlobsProtocol, Hash};
use tokio::sync::OnceCell;

use crate::code::{connection, get_within, put, CODE_ALPN, GONE, STEP, TOKIO};
use crate::code_send::CodeHandler;
use crate::device::{DeviceHandler, DeviceInbound, NodeLink, DEVICE_ALPN};
use crate::model::RelayError;

const KEY_FILE: &str = "device.key";

/// The blobs each endpoint may fetch: the current send's, for the
/// receiver that proved its code.
pub(crate) type Allowed = Arc<Mutex<HashMap<EndpointId, HashSet<Hash>>>>;

pub(crate) struct Node {
    pub endpoint: Endpoint,
    pub store: FsStore,
    /// The send that answers word codes now, if any.
    pub sending: Arc<Mutex<Option<CodeHandler>>>,
    pub allowed: Allowed,
    /// The blobs trusted devices may fetch: what sends to them offer
    /// (`device`), kept apart from the word code's so ending one doesn't
    /// end the other.
    pub device_allowed: Allowed,
    router: Router,
}

impl Node {
    /// Opens the store in `store_dir` and binds the endpoint with the key
    /// in `key_dir` (made on first use). `devices` answers trusted devices
    /// (`relay/device/1`); `link` is set to the node once it runs.
    pub async fn start(
        key_dir: &Path,
        store_dir: &Path,
        devices: Arc<dyn DeviceInbound>,
        link: NodeLink,
    ) -> Result<Self, RelayError> {
        let key = device_key(key_dir)?;
        let store = FsStore::load(store_dir).await.map_err(file_error)?;
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .map_err(|e| RelayError::Unreachable {
                reason: e.to_string(),
            })?;
        let sending = Arc::new(Mutex::new(None));
        let allowed: Allowed = Arc::default();
        let device_allowed: Allowed = Arc::default();
        let blobs = BlobsProtocol::new(
            &store,
            Some(gate(vec![
                Arc::clone(&allowed),
                Arc::clone(&device_allowed),
            ])),
        );
        let router = Router::builder(endpoint.clone())
            .accept(
                CODE_ALPN,
                Codes {
                    sending: Arc::clone(&sending),
                },
            )
            .accept(iroh_blobs::ALPN, blobs)
            .accept(
                DEVICE_ALPN,
                DeviceHandler {
                    host: devices,
                    node: link,
                },
            )
            .spawn();
        log::info!("relay node: {}", endpoint.id().fmt_short());
        Ok(Self {
            endpoint,
            store,
            sending,
            allowed,
            device_allowed,
            router,
        })
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// The UDP port the endpoint listens on (IPv4), for Bonjour.
    pub fn port(&self) -> Result<u16, RelayError> {
        self.endpoint
            .bound_sockets()
            .into_iter()
            .find(std::net::SocketAddr::is_ipv4)
            .map(|s| s.port())
            .ok_or_else(|| RelayError::Unreachable {
                reason: "no IPv4 socket".into(),
            })
    }

    /// Closes the endpoint and the store; what is stored stays on disk.
    pub async fn shutdown(&self) {
        self.router.shutdown().await.ok();
        self.store.shutdown().await.ok();
    }
}

/// The node, started on first use and kept for the core's lifetime.
pub(crate) struct NodeCell {
    key_dir: PathBuf,
    store_dir: PathBuf,
    devices: Arc<dyn DeviceInbound>,
    cell: OnceCell<Arc<Node>>,
}

impl NodeCell {
    pub fn new(key_dir: PathBuf, store_dir: PathBuf, devices: Arc<dyn DeviceInbound>) -> Self {
        Self {
            key_dir,
            store_dir,
            devices,
            cell: OnceCell::new(),
        }
    }

    /// The running node. Call on Relay's runtime (`TOKIO`).
    pub async fn get(&self) -> Result<Arc<Node>, RelayError> {
        self.cell
            .get_or_try_init(|| async {
                let link = NodeLink::default();
                let node = Arc::new(
                    Node::start(
                        &self.key_dir,
                        &self.store_dir,
                        Arc::clone(&self.devices),
                        Arc::clone(&link),
                    )
                    .await?,
                );
                let _ = link.set(Arc::downgrade(&node));
                Ok(node)
            })
            .await
            .cloned()
    }
}

impl Drop for NodeCell {
    fn drop(&mut self) {
        if let Some(node) = self.cell.take() {
            TOKIO.spawn(async move { node.shutdown().await });
        }
    }
}

/// This device's key: read from `dir/device.key`, or made and saved there
/// (readable by this user only) the first time.
pub(crate) fn device_key(dir: &Path) -> Result<SecretKey, RelayError> {
    let path = dir.join(KEY_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| RelayError::Corrupt {
                reason: format!("{KEY_FILE} isn't a key"),
            })?;
            return Ok(SecretKey::from_bytes(&bytes));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(file_error(e)),
    }
    let bytes = forge_devproto::new_secret();
    std::fs::create_dir_all(dir).map_err(file_error)?;
    let temporary = dir.join(format!("{KEY_FILE}.new"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(file_error)?;
    file.write_all(&bytes).map_err(file_error)?;
    file.sync_all().map_err(file_error)?;
    std::fs::rename(&temporary, &path).map_err(file_error)?;
    Ok(SecretKey::from_bytes(&bytes))
}

pub(crate) fn file_error(e: impl std::fmt::Display) -> RelayError {
    RelayError::File {
        reason: e.to_string(),
    }
}

/// Word-code connections go to the current send; with none running, the
/// caller hears that the code is gone.
#[derive(Debug, Clone)]
struct Codes {
    sending: Arc<Mutex<Option<CodeHandler>>>,
}

impl ProtocolHandler for Codes {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let current = lock(&self.sending).clone();
        match current {
            Some(handler) => return handler.accept(conn).await,
            None => {
                if let Err(err) = turn_away(&conn).await {
                    log::info!("code: turning away failed: {err}");
                }
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(2), conn.closed()).await;
                conn.close(0u32.into(), b"bye");
            }
        }
        Ok(())
    }
}

async fn turn_away(conn: &Connection) -> Result<(), RelayError> {
    let (mut send, mut recv) = tokio::time::timeout(STEP, conn.accept_bi())
        .await
        .map_err(|_| connection("no stream"))?
        .map_err(connection)?;
    get_within(&mut recv, STEP).await?;
    put(&mut send, &[GONE]).await?;
    send.finish().ok();
    Ok(())
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Lets an endpoint fetch only the blobs one of `allowed` allows it (see
/// [`Allowed`]); asking for anything else, or for several at once, is
/// refused.
fn gate(allowed: Vec<Allowed>) -> EventSender {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        get: RequestMode::Intercept,
        get_many: RequestMode::Disabled,
        observe: ObserveMode::Intercept,
        ..EventMask::DEFAULT
    };
    let (events, mut rx) = EventSender::channel(32, mask);
    tokio::spawn(async move {
        // Which endpoint each blobs connection belongs to.
        let mut who: HashMap<u64, EndpointId> = HashMap::new();
        let may = |who: &HashMap<u64, EndpointId>, connection: u64, hash: &Hash| {
            who.get(&connection).is_some_and(|id| {
                allowed
                    .iter()
                    .any(|a| lock(a).get(id).is_some_and(|h| h.contains(hash)))
            })
        };
        let answer = |ok: bool| {
            if ok {
                Ok(())
            } else {
                Err(AbortReason::Permission)
            }
        };
        while let Some(msg) = rx.recv().await {
            match msg {
                ProviderMessage::ClientConnected(msg) => {
                    let id = msg.inner.endpoint_id;
                    let ok = id.is_some_and(|id| allowed.iter().any(|a| lock(a).contains_key(&id)));
                    if let (true, Some(id)) = (ok, id) {
                        who.insert(msg.inner.connection_id, id);
                    }
                    msg.tx.send(answer(ok)).await.ok();
                }
                ProviderMessage::ConnectionClosed(msg) => {
                    who.remove(&msg.inner.connection_id);
                }
                ProviderMessage::GetRequestReceived(msg) => {
                    let ok = may(&who, msg.inner.connection_id, &msg.inner.request.hash);
                    msg.tx.send(answer(ok)).await.ok();
                }
                ProviderMessage::ObserveRequestReceived(msg) => {
                    let ok = may(&who, msg.inner.connection_id, &msg.inner.request.hash);
                    msg.tx.send(answer(ok)).await.ok();
                }
                _ => {}
            }
        }
    });
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = device_key(dir.path()).unwrap();
        let again = device_key(dir.path()).unwrap();
        assert_eq!(first.public(), again.public());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_damaged_key_is_an_error_not_a_new_identity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(KEY_FILE), b"short").unwrap();
        assert!(matches!(
            device_key(dir.path()),
            Err(RelayError::Corrupt { .. })
        ));
    }

    #[test]
    fn the_endpoint_id_survives_a_relaunch() {
        let dir = tempfile::tempdir().unwrap();
        TOKIO.block_on(async {
            let a = Node::start(
                dir.path(),
                &dir.path().join("store"),
                Arc::new(crate::device::NoDevices),
                NodeLink::default(),
            )
            .await
            .unwrap();
            let id = a.id();
            a.shutdown().await;
            let b = Node::start(
                dir.path(),
                &dir.path().join("store"),
                Arc::new(crate::device::NoDevices),
                NodeLink::default(),
            )
            .await
            .unwrap();
            assert_eq!(b.id(), id);
            b.shutdown().await;
        });
    }
}
