//! This device on iroh: one endpoint with a key that lasts, and one blob
//! store on disk (Relay M1, [relay-m1-m2-brief.md], "One transfer path").
//!
//! - **The key** is this device's identity. It is made once and reused on
//!   every launch, so the endpoint id never changes under a connection
//!   already made. On the iPhone it lives in the Keychain, "this device
//!   only" (the host's secure store, `ForgeServices.install()`), so it never
//!   leaves the phone in a backup: a phone restored from a backup is a new
//!   device, to be trusted again (relay-review-2026-11.md, P4). An older
//!   install's `device.key` moves into the Keychain unchanged, so trusted
//!   devices keep working, and the file is then removed. Where the host has
//!   no secure store (Linux, the `relay` command) the key stays in
//!   `device.key` in Relay's data directory (0600).
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
use std::sync::atomic::{AtomicU64, Ordering};
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
use crate::resolver::RelayServers;

const KEY_FILE: &str = "device.key";
/// The UDP port the endpoint last listened on, so a device that remembers
/// this one's address (over Tailscale, say) finds it there again.
const PORT_FILE: &str = "device.port";

/// How often the store drops what nothing holds any more (a received
/// file's copy once it is saved, a sent file's once the send ends).
static CLEANUP_INTERVAL_MS: AtomicU64 = AtomicU64::new(if cfg!(test) { 300 } else { 30_000 });
/// A receive that was neither finished nor resumed for this long lets go of
/// what arrived, so a transfer that never comes back doesn't hold space for
/// ever.
pub(crate) const RECEIVE_KEPT_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// For tests: clean the store more often. Takes effect for stores opened
/// afterwards.
#[doc(hidden)]
pub fn set_store_cleanup_interval(interval: std::time::Duration) {
    CLEANUP_INTERVAL_MS.store(interval.as_millis() as u64, Ordering::SeqCst);
}

/// The blob store, with garbage collection on. What stays is what a tag
/// holds: a receive's named tag (`relay/receive/...`, `code::hold`) until
/// the file is saved, and a send's temporary tags while it runs. One tag
/// per receive, on its root, never one per blob.
async fn open_store(store_dir: &Path) -> Result<FsStore, RelayError> {
    let mut options = iroh_blobs::store::fs::options::Options::new(store_dir);
    options.gc = Some(iroh_blobs::store::GcConfig {
        interval: std::time::Duration::from_millis(CLEANUP_INTERVAL_MS.load(Ordering::SeqCst)),
        add_protected: None,
    });
    let store = FsStore::load_with_opts(store_dir.join("blobs.db"), options)
        .await
        .map_err(file_error)?;
    crate::code::let_go_of_old_receives(&store).await;
    Ok(store)
}

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
        servers: &RelayServers,
    ) -> Result<Self, RelayError> {
        let key = load_key(key_dir)?;
        let store = open_store(store_dir).await?;
        let endpoint = bind(key, key_dir, servers).await?;
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

    /// Where this device can be reached ("host:port"), for the Hello that
    /// tells a trusted device: every interface's address, Tailscale's too.
    pub fn addresses(&self) -> Vec<String> {
        self.endpoint
            .addr()
            .ip_addrs()
            .filter(|a| crate::resolver::worth_telling(a))
            .take(crate::resolver::MAX_ADDRESSES)
            .map(|a| a.to_string())
            .collect()
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
    /// Relay's servers, as they were when the node started (a change takes
    /// effect the next time Relay opens).
    servers: RelayServers,
    cell: OnceCell<Arc<Node>>,
}

impl NodeCell {
    pub fn new(
        key_dir: PathBuf,
        store_dir: PathBuf,
        devices: Arc<dyn DeviceInbound>,
        servers: RelayServers,
    ) -> Self {
        Self {
            key_dir,
            store_dir,
            devices,
            servers,
            cell: OnceCell::new(),
        }
    }

    /// The node, if it has started.
    pub fn started(&self) -> Option<Arc<Node>> {
        self.cell.get().cloned()
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
                        &self.servers,
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

/// Binds the endpoint on the port it had last time, if it is free, else
/// on any port, and remembers the port.
async fn bind(
    key: SecretKey,
    key_dir: &Path,
    servers: &RelayServers,
) -> Result<Endpoint, RelayError> {
    use iroh::address_lookup::pkarr::{PkarrPublisher, PkarrResolver};
    use iroh::endpoint_info::AddrFilter;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    let relay = servers.relay();
    let lookup = servers.lookup();
    let builder = || {
        let mut b = Endpoint::builder(presets::Minimal).secret_key(key.clone());
        b = match &relay {
            Some(url) => b.relay_mode(RelayMode::Custom(iroh::RelayMap::from(url.clone()))),
            None => b.relay_mode(RelayMode::Disabled),
        };
        if let Some(url) = &lookup {
            // Our own server: it gets the relay and any public address,
            // never home-network or Tailscale ones (`publishable`).
            b =
                b.address_lookup(PkarrPublisher::builder(url.clone()).addr_filter(
                    AddrFilter::new(|a| std::borrow::Cow::Owned(crate::resolver::publishable(a))),
                ))
                .address_lookup(PkarrResolver::builder(url.clone()));
        }
        b
    };
    let saved = std::fs::read_to_string(key_dir.join(PORT_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p != 0);
    let mut endpoint = None;
    if let Some(port) = saved {
        let pinned = builder()
            .clear_ip_transports()
            .bind_addr(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
            .and_then(|b| {
                b.bind_addr_with_opts(
                    SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
                    iroh::endpoint::BindOpts::default().set_is_required(false),
                )
            });
        match pinned {
            Ok(b) => match b.bind().await {
                Ok(e) => endpoint = Some(e),
                Err(e) => log::info!("relay node: port {port} isn't free ({e}); using another"),
            },
            Err(e) => log::info!("relay node: can't ask for port {port}: {e}"),
        }
    }
    let endpoint = match endpoint {
        Some(e) => e,
        None => builder()
            .bind()
            .await
            .map_err(|e| RelayError::Unreachable {
                reason: e.to_string(),
            })?,
    };
    if let Some(port) = endpoint
        .bound_sockets()
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .map(|s| s.port())
    {
        if saved != Some(port) {
            let _ = std::fs::create_dir_all(key_dir);
            if let Err(e) = std::fs::write(key_dir.join(PORT_FILE), port.to_string()) {
                log::info!("relay node: couldn't remember the port: {e}");
            }
        }
    }
    Ok(endpoint)
}

/// The secure-store account the device key lives under where the host has
/// a secure store (the iOS Keychain).
pub(crate) const KEY_ACCOUNT: &str = "relay.device-key";

/// Whether this platform keeps the device key only in the host's secure
/// store: the iPhone, whose data folder travels in backups.
const KEY_NEEDS_SECURE_STORE: bool = cfg!(target_os = "ios");

/// This device's key: from the host's secure store when it has installed
/// one (the iPhone's Keychain), else from `dir/device.key`.
fn load_key(dir: &Path) -> Result<SecretKey, RelayError> {
    let services = crate::native::NativeServices::installed();
    key_from(
        services.as_ref().map(|s| s.secure_store.as_ref()),
        KEY_NEEDS_SECURE_STORE,
        dir,
    )
}

/// The key from `store` if the host installed one. Without one, the file
/// on a platform that allows it, and an error on one that doesn't (the
/// iPhone): falling back to a file there would put the identity back where
/// backups copy it, so Relay doesn't start its endpoint instead.
fn key_from(
    store: Result<&dyn crate::native::SecureStore, &crate::native::NativeServiceError>,
    needs_secure_store: bool,
    dir: &Path,
) -> Result<SecretKey, RelayError> {
    match store {
        Ok(store) => device_key_in(store, dir),
        Err(e) if needs_secure_store => Err(RelayError::Storage {
            reason: format!(
                "the Keychain isn't available ({e}), and Relay keeps this device's key only there"
            ),
        }),
        Err(_) => device_key(dir),
    }
}

/// This device's key in a secure store (`KEY_ACCOUNT`). The first time,
/// the key in `dir/device.key` moves in unchanged (an older install), or a
/// new one is made; it is read back before the file is removed, since the
/// file is the only other copy. A key in the store wins over a file left
/// beside it, and that file is removed too: a copy outside the store would
/// travel in backups. A store that can't be read is an error, never a new
/// key: a new key is a new identity, and every trusted device would refuse
/// it.
pub(crate) fn device_key_in(
    store: &dyn crate::native::SecureStore,
    dir: &Path,
) -> Result<SecretKey, RelayError> {
    let file = dir.join(KEY_FILE);
    let unreadable = |what: &str| RelayError::Corrupt {
        reason: format!("{what} isn't a device key"),
    };
    let stored = store
        .get(KEY_ACCOUNT.to_string())
        .map_err(secure_store_error)?;
    let key: [u8; 32] = match stored {
        Some(bytes) => bytes
            .try_into()
            .map_err(|_| unreadable("the Keychain's copy"))?,
        None => {
            let key: [u8; 32] = match std::fs::read(&file) {
                Ok(bytes) => bytes.try_into().map_err(|_| unreadable(KEY_FILE))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => crate::hex::random(),
                Err(e) => return Err(file_error(e)),
            };
            store
                .set(KEY_ACCOUNT.to_string(), key.to_vec())
                .map_err(secure_store_error)?;
            let kept = store
                .get(KEY_ACCOUNT.to_string())
                .map_err(secure_store_error)?;
            if kept.as_deref() != Some(&key[..]) {
                return Err(RelayError::Storage {
                    reason: "the Keychain didn't keep this device's key".into(),
                });
            }
            key
        }
    };
    match std::fs::remove_file(&file) {
        Ok(()) => log::info!("device key: kept in the secure store; the file is gone"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("device key: the old file couldn't be removed: {e}"),
    }
    Ok(SecretKey::from_bytes(&key))
}

fn secure_store_error(e: crate::native::NativeServiceError) -> RelayError {
    RelayError::Storage {
        reason: format!("the device key in the Keychain: {e}"),
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
    let bytes = crate::hex::random();
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

    use crate::native::testing::MemorySecureStore;
    use crate::native::{NativeServiceError, SecureStore};

    /// A Keychain that can't be read (locked, or failing).
    struct BrokenStore;
    impl SecureStore for BrokenStore {
        fn get(&self, _: String) -> Result<Option<Vec<u8>>, NativeServiceError> {
            Err(NativeServiceError::Platform {
                code: -25308,
                reason: "Keychain reading: interaction not allowed".into(),
            })
        }
        fn set(&self, _: String, _: Vec<u8>) -> Result<(), NativeServiceError> {
            unreachable!("never written after a failed read")
        }
        fn delete(&self, _: String) -> Result<(), NativeServiceError> {
            Ok(())
        }
    }

    /// A Keychain that says yes to writes but keeps nothing.
    #[derive(Default)]
    struct ForgetfulStore;
    impl SecureStore for ForgetfulStore {
        fn get(&self, _: String) -> Result<Option<Vec<u8>>, NativeServiceError> {
            Ok(None)
        }
        fn set(&self, _: String, _: Vec<u8>) -> Result<(), NativeServiceError> {
            Ok(())
        }
        fn delete(&self, _: String) -> Result<(), NativeServiceError> {
            Ok(())
        }
    }

    fn in_store(store: &MemorySecureStore) -> Option<Vec<u8>> {
        store.get(KEY_ACCOUNT.into()).unwrap()
    }

    #[test]
    fn a_fresh_install_keeps_its_key_in_the_keychain_and_never_in_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemorySecureStore::default();
        let first = device_key_in(&keychain, dir.path()).unwrap();
        assert_eq!(in_store(&keychain).unwrap(), first.to_bytes());
        assert!(
            !dir.path().join(KEY_FILE).exists(),
            "no copy for backups to carry"
        );
        // Every later launch (and a reinstall, since the Keychain item
        // outlives the app) is the same device.
        let again = device_key_in(&keychain, dir.path()).unwrap();
        assert_eq!(first.public(), again.public());
    }

    #[test]
    fn an_upgrade_moves_the_old_key_into_the_keychain_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        // An older install: the key in a file, already trusted elsewhere.
        let old = device_key(dir.path()).unwrap();
        let keychain = MemorySecureStore::default();
        let moved = device_key_in(&keychain, dir.path()).unwrap();
        assert_eq!(old.public(), moved.public(), "trusted devices keep working");
        assert_eq!(in_store(&keychain).unwrap(), old.to_bytes());
        assert!(!dir.path().join(KEY_FILE).exists(), "the file is gone");
        let again = device_key_in(&keychain, dir.path()).unwrap();
        assert_eq!(old.public(), again.public());
    }

    #[test]
    fn a_backup_restored_onto_a_new_phone_is_a_new_device() {
        // The old phone, after the move: its key only in its own Keychain,
        // and nothing in the data a backup carries.
        let old_phone = tempfile::tempdir().unwrap();
        let old_keychain = MemorySecureStore::default();
        let old = device_key_in(&old_keychain, old_phone.path()).unwrap();
        // The backup holds the data folder, which has no key file now; the
        // new phone's Keychain doesn't have the "this device only" item.
        let restored = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(old_phone.path()).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), restored.path().join(entry.file_name())).unwrap();
        }
        let new_keychain = MemorySecureStore::default();
        let new = device_key_in(&new_keychain, restored.path()).unwrap();
        assert_ne!(
            old.public(),
            new.public(),
            "a new identity, to be trusted again"
        );
    }

    #[test]
    fn the_keychain_wins_over_a_stray_file_which_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemorySecureStore::default();
        let kept = device_key_in(&keychain, dir.path()).unwrap();
        // A file appears beside it (an old backup's data restored over this
        // install): the Keychain's key stays, and the file goes.
        std::fs::write(dir.path().join(KEY_FILE), [9u8; 32]).unwrap();
        let after = device_key_in(&keychain, dir.path()).unwrap();
        assert_eq!(kept.public(), after.public());
        assert!(!dir.path().join(KEY_FILE).exists());
    }

    #[test]
    fn a_keychain_that_cant_be_read_is_an_error_never_a_new_identity() {
        let dir = tempfile::tempdir().unwrap();
        let old = device_key(dir.path()).unwrap();
        assert!(matches!(
            device_key_in(&BrokenStore, dir.path()),
            Err(RelayError::Storage { .. })
        ));
        // Nothing was lost: the old key is still there for the next try.
        assert_eq!(device_key(dir.path()).unwrap().public(), old.public());
    }

    #[test]
    fn the_file_stays_unless_the_keychain_really_kept_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let old = device_key(dir.path()).unwrap();
        assert!(matches!(
            device_key_in(&ForgetfulStore, dir.path()),
            Err(RelayError::Storage { .. })
        ));
        assert_eq!(device_key(dir.path()).unwrap().public(), old.public());
    }

    #[test]
    fn a_damaged_key_in_the_keychain_is_an_error_not_a_new_identity() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemorySecureStore::default();
        keychain.set(KEY_ACCOUNT.into(), b"short".to_vec()).unwrap();
        assert!(matches!(
            device_key_in(&keychain, dir.path()),
            Err(RelayError::Corrupt { .. })
        ));
        assert_eq!(
            in_store(&keychain).unwrap(),
            b"short",
            "left for the person to see"
        );
    }

    #[test]
    fn without_a_secure_store_the_iphone_refuses_and_linux_uses_the_file() {
        let not_installed = NativeServiceError::NotInstalled {
            service: "core".into(),
        };
        // The iPhone: no Keychain means no endpoint, and no file is made.
        let phone = tempfile::tempdir().unwrap();
        assert!(matches!(
            key_from(Err(&not_installed), true, phone.path()),
            Err(RelayError::Storage { .. })
        ));
        assert!(!phone.path().join(KEY_FILE).exists());
        // Linux: the 0600 file, as always.
        let linux = tempfile::tempdir().unwrap();
        let key = key_from(Err(&not_installed), false, linux.path()).unwrap();
        assert_eq!(device_key(linux.path()).unwrap().public(), key.public());
        // With a store, both use it.
        let keychain = MemorySecureStore::default();
        let a = key_from(Ok(&keychain), true, phone.path()).unwrap();
        assert_eq!(in_store(&keychain).unwrap(), a.to_bytes());
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
                &RelayServers::default(),
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
                &RelayServers::default(),
            )
            .await
            .unwrap();
            assert_eq!(b.id(), id);
            b.shutdown().await;
        });
    }
}
