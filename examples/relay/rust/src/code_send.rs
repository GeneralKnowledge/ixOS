//! Sending a file with a word code (Relay M1): the M0 spike's sender, moved
//! into relay-core so the iPhone can send too. Same wire protocol as
//! `code::receive` and the spike, same rules ([`crate::code_session`]):
//!
//! - The file is hashed where it is (iroh-blobs, BLAKE3) and offered on an
//!   iroh endpoint with relays off: the local network only.
//! - A code is a number nobody nearby is using and three words. The app
//!   advertises the number and this endpoint's id over Bonjour (Rust does
//!   not do multicast on iOS) and reports the other senders it sees, so a
//!   new code avoids their numbers and a clash before the code is used
//!   replaces it.
//! - One handshake at a time. A wrong guess spends the code and a new one
//!   is issued at once; so does a dropped transfer, and the receiver
//!   resumes with it.
//! - Only an endpoint that proved the code may fetch the file.
//!
//! The app follows what happens by reading [`CodeSend`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::AcceptError;
use iroh::EndpointId;
use iroh_blobs::api::blobs::{AddPathOptions, ImportMode};
use iroh_blobs::api::TempTag;
use iroh_blobs::format::collection::Collection;
use iroh_blobs::{BlobFormat, Hash};
use tokio::sync::mpsc;

use crate::code::{
    confirm, connection, get_within, put, WordCode, BUSY, COLLECTION, GO, GONE, STEP, TOKIO,
};
use crate::code_session::{Answer, Ending, Session as Machine};
use crate::model::RelayError;
use crate::node::{file_error, lock, Node, NodeCell};

/// How long the receiver may go without a tick before the transfer counts
/// as dropped (QUIC alone can take minutes to notice).
const SILENCE: Duration = Duration::from_secs(30);
/// How long an unused code can be redeemed.
const CODE_LIFETIME: Duration = Duration::from_secs(600);
/// New codes after wrong guesses, drops or clashes before the sender stops.
const MAX_CODES: usize = 6;

/// Where a send with a code has got. The app shows it and, while there is a
/// code, advertises `number`, `port` and `endpoint_id` with Bonjour
/// (`CODE_SERVICE_TYPE`, TXT `v=1`, `n` = number, `id` = endpoint id).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum CodeSend {
    Idle,
    /// Hashing the file.
    Preparing {
        name: String,
        size: u64,
    },
    /// The code is ready for the receiver. `note` says why the code changed,
    /// when it did.
    Waiting {
        code: String,
        number: u8,
        port: u16,
        endpoint_id: String,
        name: String,
        size: u64,
        note: Option<String>,
    },
    /// A receiver proved the code and is fetching the file.
    Sending {
        code: String,
        number: u8,
        port: u16,
        endpoint_id: String,
        name: String,
        size: u64,
    },
    /// The receiver has the file, verified and saved.
    Sent {
        name: String,
        size: u64,
        seconds: f64,
        megabytes_per_second: f64,
    },
    Failed {
        reason: String,
    },
}

/// Another sender the app sees advertising nearby.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NearbySender {
    pub number: u8,
    pub endpoint_id: String,
}

/// What a send shares with the app: its status, the senders seen nearby,
/// and the stop switch.
#[derive(Default)]
pub struct CodeSendShared {
    pub status: Mutex<Option<CodeSend>>,
    pub nearby: Mutex<HashSet<NearbySender>>,
    pub stop: AtomicBool,
    /// This device's name, told to the receiver once it has the files.
    pub my_name: Mutex<String>,
    /// Who received the last send: its endpoint id (who proved the code)
    /// and what it calls itself. Set when the send succeeds.
    pub receiver: Mutex<Option<(String, String)>>,
}

impl CodeSendShared {
    pub fn status(&self) -> CodeSend {
        lock(&self.status).clone().unwrap_or(CodeSend::Idle)
    }

    fn set(&self, status: CodeSend) {
        *lock(&self.status) = Some(status);
    }

    /// Whether a send is preparing, waiting or sending.
    pub fn running(&self) -> bool {
        matches!(
            self.status(),
            CodeSend::Preparing { .. } | CodeSend::Waiting { .. } | CodeSend::Sending { .. }
        )
    }

    fn numbers_in_use(&self, me: EndpointId) -> Vec<u8> {
        let me = me.to_string();
        lock(&self.nearby)
            .iter()
            .filter(|s| s.endpoint_id != me)
            .map(|s| s.number)
            .collect()
    }
}

/// Called once when a send ends, with its last status (`Sent` or `Failed`).
pub(crate) type OnEnd = Box<dyn FnOnce(&CodeSend) + Send>;

/// What a send of `paths` is called: the one item's name, or how many.
pub(crate) fn send_name(paths: &[PathBuf]) -> String {
    match paths {
        [one] => one
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string(),
        many => format!("{} items", many.len()),
    }
}

/// Starts sending `paths` (files and folders, as one transfer) with a word
/// code in the background; follow it in `shared.status`. The node's store
/// keeps the hashes, so sending the same thing again need not hash it again.
pub(crate) fn start(
    paths: Vec<PathBuf>,
    node: Arc<NodeCell>,
    shared: Arc<CodeSendShared>,
    on_end: OnEnd,
) {
    shared.stop.store(false, Ordering::SeqCst);
    lock(&shared.receiver).take();
    // Set before returning, so the app's first read already sees the send
    // running: it stops polling once the status is no longer running.
    let name = send_name(&paths);
    // Files' sizes now; a folder's once it has been walked.
    let size = paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    shared.set(CodeSend::Preparing {
        name: name.clone(),
        size,
    });
    TOKIO.spawn(async move {
        let outcome = match node.get().await {
            Ok(node) => {
                let outcome = send(&paths, name, &node, &shared).await;
                *lock(&node.sending) = None;
                lock(&node.allowed).clear();
                outcome
            }
            Err(err) => Err(err),
        };
        if let Err(err) = outcome {
            log::info!("code send ended: {err}");
            shared.set(CodeSend::Failed {
                reason: err.to_string(),
            });
        }
        on_end(&shared.status());
    });
}

/// What a send offers: one file (a raw blob) or a folder (a collection).
#[derive(Clone)]
pub(crate) struct Offer {
    pub hash: Hash,
    pub collection: bool,
    pub size: u64,
    pub name: String,
    /// Every blob the receiver may fetch: the root, and for a folder its
    /// list and files.
    pub hashes: HashSet<Hash>,
}

/// The live code and its state machine.
struct Session {
    code: WordCode,
    issued: Instant,
    machine: Machine,
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    /// The receiver has the file, `.0` after the offer.
    Done(Duration),
    WrongCode,
    Dropped,
    Collision,
}

/// Something that happened to code generation `gen`.
#[derive(Debug)]
struct Event {
    gen: u64,
    kind: Kind,
}

/// The running send, as the node's word-code protocol sees it.
#[derive(Clone)]
pub(crate) struct CodeHandler {
    me: EndpointId,
    port: u16,
    session: Arc<Mutex<Session>>,
    offer: Offer,
    allowed: crate::node::Allowed,
    events: mpsc::Sender<Event>,
    shared: Arc<CodeSendShared>,
    /// Open code connections, closed when the send ends so a receiver
    /// stops at once.
    live: Arc<Mutex<Vec<Connection>>>,
}

impl std::fmt::Debug for CodeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CodeHandler")
    }
}

impl CodeHandler {
    /// `None` when the attempt was turned away without spending the code.
    /// Once admitted, every way the attempt can end produces an event, so a
    /// spent code is always replaced.
    async fn run(&self, conn: &Connection) -> Result<Option<Event>, RelayError> {
        let them = conn.remote_id();
        let (mut send, mut recv) = tokio::time::timeout(STEP, conn.accept_bi())
            .await
            .map_err(|_| connection("no stream"))?
            .map_err(connection)?;
        let their_msg = get_within(&mut recv, STEP).await?;
        let admitted = {
            let mut s = lock(&self.session);
            if s.issued.elapsed() > CODE_LIFETIME {
                s.machine.expire();
            }
            match s.machine.attempt() {
                (Answer::Go, gen) => Ok((s.code.clone(), gen)),
                (Answer::Busy, _) => Err(BUSY),
                (Answer::Gone, _) => Err(GONE),
            }
        };
        let (code, gen) = match admitted {
            Ok(admitted) => admitted,
            Err(refusal) => {
                put(&mut send, &[refusal]).await.ok();
                send.finish().ok();
                return Ok(None);
            }
        };
        let ending = self
            .authenticate(&mut send, &mut recv, &code, &their_msg, them)
            .await;
        let accepted = lock(&self.session).machine.finish(gen, ending);
        let kind = match ending {
            Ending::WrongCode => Kind::WrongCode,
            Ending::Dropped => Kind::Dropped,
            Ending::Proved if !accepted => Kind::Dropped,
            Ending::Proved => {
                lock(&self.allowed).insert(them, self.offer.hashes.clone());
                self.shared.set(CodeSend::Sending {
                    code: code.text(),
                    number: code.number,
                    port: self.port,
                    endpoint_id: self.me.to_string(),
                    name: self.offer.name.clone(),
                    size: self.offer.size,
                });
                let kind = self.deliver(&mut send, &mut recv).await;
                if matches!(kind, Kind::Done(_)) {
                    // Names last, once the receiver has it all (code.rs).
                    let name = crate::code::their_name(&mut recv).await;
                    let mine = lock(&self.shared.my_name).clone();
                    let _ = put(&mut send, &crate::code::named(&mine)).await;
                    send.finish().ok();
                    // Until the receiver has read it and finished its side:
                    // the send closes the connection as soon as this returns.
                    let _ =
                        tokio::time::timeout(Duration::from_secs(2), recv.read_to_end(64)).await;
                    *lock(&self.shared.receiver) = Some((them.to_string(), name));
                }
                kind
            }
        };
        Ok(Some(Event { gen, kind }))
    }

    /// SPAKE2 and key confirmation. A malformed or wrong proof, or none in
    /// time, is a wrong code; failing to send is a dropped connection.
    async fn authenticate(
        &self,
        send: &mut SendStream,
        recv: &mut RecvStream,
        code: &WordCode,
        their_msg: &[u8],
        them: EndpointId,
    ) -> Ending {
        let (state, my_msg) = code.pake();
        let mut first = vec![GO];
        first.extend_from_slice(&my_msg);
        if put(send, &first).await.is_err() {
            return Ending::Dropped;
        }
        let Ok(key) = state.finish(their_msg) else {
            return Ending::WrongCode;
        };
        match get_within(recv, STEP).await {
            Ok(theirs) if theirs == confirm(&key, "receiver", self.me, them) => {}
            _ => return Ending::WrongCode,
        }
        if put(send, &confirm(&key, "sender", self.me, them))
            .await
            .is_err()
        {
            return Ending::Dropped;
        }
        Ending::Proved
    }

    /// Names the file and waits until the receiver has it.
    async fn deliver(&self, send: &mut SendStream, recv: &mut RecvStream) -> Kind {
        let started = Instant::now();
        // A file's offer is the spike's (hash, size, name), so
        // `relay-spike` still talks to it; a folder's puts 0 and the format
        // before the name, which no name starts with.
        let mut offer = Vec::new();
        offer.extend_from_slice(self.offer.hash.as_bytes());
        offer.extend_from_slice(&self.offer.size.to_be_bytes());
        if self.offer.collection {
            offer.extend_from_slice(&[0, COLLECTION]);
        }
        offer.extend_from_slice(self.offer.name.as_bytes());
        if put(send, &offer).await.is_err() {
            return Kind::Dropped;
        }
        // The receiver ticks every couple of seconds while the file moves,
        // then says "done" once it is verified and saved.
        loop {
            match get_within(recv, SILENCE).await {
                Ok(m) if m == b"tick" => continue,
                Ok(m) if m == b"done" => return Kind::Done(started.elapsed()),
                Ok(_) => return Kind::Dropped,
                Err(_) => {
                    log::info!("code send: receiver silent for {}s", SILENCE.as_secs());
                    return Kind::Dropped;
                }
            }
        }
    }
}

impl CodeHandler {
    pub(crate) async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        lock(&self.live).push(connection.clone());
        match self.run(&connection).await {
            Ok(Some(event)) => {
                let _ = self.events.send(event).await;
            }
            Ok(None) => {}
            Err(err) => log::info!("code send: a connection failed: {err}"),
        }
        // Let the other side read our last words before closing.
        let _ = tokio::time::timeout(Duration::from_secs(2), connection.closed()).await;
        connection.close(0u32.into(), b"bye");
        lock(&self.live).retain(|c| c.stable_id() != connection.stable_id());
        Ok(())
    }
}

async fn send(
    paths: &[PathBuf],
    name: String,
    node: &Arc<Node>,
    shared: &Arc<CodeSendShared>,
) -> Result<(), RelayError> {
    let started = Instant::now();
    // Held for the whole send, so what it offers stays in the store.
    let (offer, _tags) = prepare(paths, name.clone(), node).await?;
    log::info!(
        "code send: hashed {} bytes in {:.1}s",
        offer.size,
        started.elapsed().as_secs_f64()
    );
    let size = offer.size;
    let me = node.id();
    let port = node.port()?;
    let session = Arc::new(Mutex::new(Session {
        code: WordCode::new(&shared.numbers_in_use(me)),
        issued: Instant::now(),
        machine: Machine::new(),
    }));
    let (tx, mut events) = mpsc::channel(8);
    let handler = CodeHandler {
        me,
        port,
        session: Arc::clone(&session),
        offer,
        allowed: Arc::clone(&node.allowed),
        events: tx,
        shared: Arc::clone(shared),
        live: Arc::default(),
    };
    let live = Arc::clone(&handler.live);
    *lock(&node.sending) = Some(handler);
    let waiting = |code: &WordCode, note: Option<String>| CodeSend::Waiting {
        code: code.text(),
        number: code.number,
        port,
        endpoint_id: me.to_string(),
        name: name.clone(),
        size,
        note,
    };
    shared.set(waiting(&lock(&session).code, None));
    log::info!("code send: waiting on port {port}");

    let mut codes = 1;
    // One clash report per code.
    let mut reported: Option<u64> = None;
    let result = loop {
        let event = match tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => break Err(connection("stopped")),
            Err(_) => {
                if shared.stop.load(Ordering::SeqCst) {
                    break Err(connection("cancelled"));
                }
                let mut s = lock(&session);
                // The lifetime limits how long an unused code can be
                // redeemed; once a receiver is in, the transfer takes as long
                // as it needs, ending with done or after SILENCE.
                if !s.machine.spent && s.issued.elapsed() > CODE_LIFETIME && s.machine.lapse() {
                    break Err(RelayError::WordCode {
                        reason: "the code expired; send again for a new one".into(),
                    });
                }
                let clash = !s.machine.spent
                    && reported != Some(s.machine.gen)
                    && shared.numbers_in_use(me).contains(&s.code.number);
                if !clash {
                    continue;
                }
                reported = Some(s.machine.gen);
                let gen = s.machine.gen;
                drop(s);
                Event {
                    gen,
                    kind: Kind::Collision,
                }
            }
        };
        let stale = {
            let mut s = lock(&session);
            match event.kind {
                Kind::Done(_) => false,
                // Checked and acted on under one lock: a clash seen while
                // the code was unused must not cut off a receiver admitted
                // since.
                Kind::Collision => !s.machine.collided(event.gen),
                _ => !s.machine.is_current(event.gen),
            }
        };
        if stale {
            continue;
        }
        let why = match event.kind {
            Kind::Done(took) => break Ok(took),
            Kind::WrongCode => "Someone tried a wrong code, so it no longer works.",
            Kind::Dropped => "The transfer stopped before the end; the new code resumes it.",
            Kind::Collision => "Another sender nearby took the same number.",
        };
        if codes >= MAX_CODES {
            break Err(RelayError::Declined {
                reason: format!("{why} Too many new codes, so the send stopped."),
            });
        }
        codes += 1;
        let code = WordCode::new(&shared.numbers_in_use(me));
        {
            let mut s = lock(&session);
            s.code = code.clone();
            s.issued = Instant::now();
            s.machine.reissue();
        }
        log::info!("code send: new code ({:?})", event.kind);
        shared.set(waiting(&code, Some(why.to_string())));
    };
    *lock(&node.sending) = None;
    lock(&node.allowed).clear();
    for conn in lock(&live).drain(..) {
        conn.close(0u32.into(), b"stopped");
    }
    let took = result?;
    let seconds = took.as_secs_f64();
    shared.set(CodeSend::Sent {
        name,
        size,
        seconds,
        megabytes_per_second: if seconds > 0.0 {
            size as f64 / seconds / 1e6
        } else {
            0.0
        },
    });
    log::info!("code send: sent in {seconds:.1}s");
    Ok(())
}

/// Hashes what `paths` hold into the store, where it lies (nothing is
/// copied): one file as one blob; anything else as a collection of files
/// with their paths inside the transfer (a lone folder's files relative to
/// it; several items each under its own name). Returns the offer and the
/// tags that keep it.
pub(crate) async fn prepare(
    paths: &[PathBuf],
    name: String,
    node: &Node,
) -> Result<(Offer, Vec<TempTag>), RelayError> {
    let [path] = paths else {
        return prepare_files(items_in(paths).map_err(file_error)?, name, node).await;
    };
    let meta = std::fs::metadata(path).map_err(file_error)?;
    if meta.is_file() {
        let tag = import(node, path.to_path_buf()).await?;
        let hash = tag.hash();
        let offer = Offer {
            hash,
            collection: false,
            size: meta.len(),
            name,
            hashes: HashSet::from([hash]),
        };
        return Ok((offer, vec![tag]));
    }
    prepare_files(files_in(path).map_err(file_error)?, name, node).await
}

/// A file's blob, hashed where it lies.
async fn import(node: &Node, file: PathBuf) -> Result<TempTag, RelayError> {
    node.store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: std::path::absolute(file).map_err(file_error)?,
            format: BlobFormat::Raw,
            // Hash the file where it is instead of copying it.
            mode: ImportMode::TryReference,
        })
        .temp_tag()
        .await
        .map_err(file_error)
}

/// The collection of `files` (path inside the transfer, file, size).
async fn prepare_files(
    files: Vec<(String, PathBuf, u64)>,
    name: String,
    node: &Node,
) -> Result<(Offer, Vec<TempTag>), RelayError> {
    let import = |file: PathBuf| import(node, file);
    if files.is_empty() {
        return Err(RelayError::File {
            reason: "there are no files to send".into(),
        });
    }
    let mut tags = Vec::with_capacity(files.len() + 1);
    let mut collection = Collection::default();
    let mut size = 0;
    for (inside, file, length) in files {
        let tag = import(file).await?;
        collection.push(inside, tag.hash());
        size += length;
        tags.push(tag);
    }
    let root = collection.store(&node.store).await.map_err(file_error)?;
    let hash = root.hash();
    // The receiver fetches the root with its children in one request; the
    // list and files are allowed too in case it asks for them one by one.
    let mut hashes: HashSet<Hash> = tags.iter().map(TempTag::hash).collect();
    hashes.insert(hash);
    if let Ok(list) = node.store.get_bytes(hash).await {
        if let Ok(seq) = iroh_blobs::hashseq::HashSeq::try_from(list) {
            hashes.extend(seq.iter());
        }
    }
    tags.push(root);
    let offer = Offer {
        hash,
        collection: true,
        size,
        name,
        hashes,
    };
    Ok((offer, tags))
}

/// The files of several items sent together: a file under its name, a
/// folder's files under the folder's name. Two items with one name are an
/// error rather than one hiding the other.
pub(crate) fn items_in(paths: &[PathBuf]) -> std::io::Result<Vec<(String, PathBuf, u64)>> {
    let mut found = Vec::new();
    let mut names = HashSet::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| std::io::Error::other(format!("{} has no name", path.display())))?
            .to_string();
        if !names.insert(name.clone()) {
            return Err(std::io::Error::other(format!(
                "two of the items are called {name}"
            )));
        }
        let meta = std::fs::metadata(path)?;
        if meta.is_dir() {
            for (inside, file, size) in files_in(path)? {
                found.push((format!("{name}/{inside}"), file, size));
            }
        } else {
            found.push((name, path.clone(), meta.len()));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

/// The files under `root` with their paths inside it (`/`-separated) and
/// sizes, sorted. Symbolic links are left out, so a send never reaches
/// outside the folder.
pub(crate) fn files_in(root: &Path) -> std::io::Result<Vec<(String, PathBuf, u64)>> {
    let mut found = Vec::new();
    let mut folders = vec![(root.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let Some(name) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let inside = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() {
                folders.push((entry.path(), inside));
            } else if kind.is_file() {
                found.push((inside, entry.path(), entry.metadata()?.len()));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::code::{receive, CodeProgress, CodeReceipt, Failed};

    async fn wait_for(shared: &CodeSendShared, f: impl Fn(&CodeSend) -> bool) -> CodeSend {
        for _ in 0..600 {
            let s = shared.status();
            if f(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("status never arrived: {:?}", shared.status());
    }

    fn address(status: &CodeSend) -> (String, String, String) {
        match status {
            CodeSend::Waiting {
                code,
                port,
                endpoint_id,
                ..
            } => (
                code.clone(),
                format!("127.0.0.1:{port}"),
                endpoint_id.clone(),
            ),
            other => panic!("not waiting: {other:?}"),
        }
    }

    /// A device for a test: its own key and store under `dir/name`.
    fn device(dir: &Path, name: &str) -> Arc<NodeCell> {
        let home = dir.join(name);
        Arc::new(NodeCell::new(
            home.clone(),
            home.join("store"),
            Arc::new(crate::device::NoDevices),
        ))
    }

    fn sending(path: PathBuf, node: &Arc<NodeCell>) -> Arc<CodeSendShared> {
        sending_all(vec![path], node)
    }

    fn sending_all(paths: Vec<PathBuf>, node: &Arc<NodeCell>) -> Arc<CodeSendShared> {
        let shared = Arc::new(CodeSendShared::default());
        start(
            paths,
            Arc::clone(node),
            Arc::clone(&shared),
            Box::new(|_| {}),
        );
        shared
    }

    async fn take(
        status: &CodeSend,
        node: &Arc<NodeCell>,
        inbox: &Path,
        progress: Arc<CodeProgress>,
    ) -> Result<CodeReceipt, Failed> {
        let (code, addr, id) = address(status);
        receive(
            &code,
            &[addr],
            &id,
            node.get().await.unwrap(),
            inbox,
            "b".into(),
            progress,
            Arc::new(AtomicBool::new(false)),
        )
        .await
    }

    fn bytes(n: u32, seed: u32) -> Vec<u8> {
        (0..n).map(|i| ((i * 7 + seed) % 251) as u8).collect()
    }

    /// The whole flow on one machine: a wrong code is spent and replaced,
    /// the new code delivers the file byte for byte.
    #[test]
    fn send_and_receive_with_a_code_over_loopback() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.bin");
        let content = bytes(3_000_000, 0);
        std::fs::write(&file, &content).unwrap();
        let (a, b) = (device(dir.path(), "a"), device(dir.path(), "b"));
        let shared = sending(file, &a);
        let inbox = dir.path().join("inbox");

        TOKIO.block_on(async {
            let first = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
            let (code, addr, id) = address(&first);
            let number: u8 = code.split('-').next().unwrap().parse().unwrap();
            let wrong = WordCode::new(&[]).text();
            let wrong = format!("{number}{}", &wrong[wrong.find('-').unwrap()..]);
            let refused = receive(
                &wrong,
                std::slice::from_ref(&addr),
                &id,
                b.get().await.unwrap(),
                &inbox,
                "b".into(),
                Arc::new(CodeProgress::default()),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap_err();
            assert!(refused.offered.is_none(), "nothing was offered yet");

            let second = wait_for(&shared, |s| {
                matches!(s, CodeSend::Waiting { note: Some(_), .. })
            })
            .await;
            let receipt = take(&second, &b, &inbox, Arc::default()).await.unwrap();
            assert_eq!(std::fs::read(&receipt.path).unwrap(), content);
            let sent = wait_for(&shared, |s| matches!(s, CodeSend::Sent { .. })).await;
            assert!(matches!(
                sent,
                CodeSend::Sent {
                    size: 3_000_000,
                    ..
                }
            ));
        });
    }

    fn folder(root: &Path) -> Vec<(&'static str, Vec<u8>)> {
        let files = vec![
            ("notes.txt", b"hello".to_vec()),
            ("photos/a.jpg", bytes(400_000, 1)),
            ("photos/deeper/b.raw", bytes(900_000, 2)),
            ("empty.bin", Vec::new()),
        ];
        for (inside, data) in &files {
            let path = root.join(inside);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        }
        files
    }

    /// A folder goes one way, then another folder the other way, between
    /// two devices: every file arrives under one folder, byte for byte.
    #[test]
    fn a_folder_goes_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (device(dir.path(), "a"), device(dir.path(), "b"));
        let there = dir.path().join("Holiday");
        let back = dir.path().join("Reply");
        let files = folder(&there);
        std::fs::create_dir_all(&back).unwrap();
        std::fs::write(back.join("thanks.txt"), b"thanks").unwrap();

        TOKIO.block_on(async {
            for (from, to, what, inbox) in [
                (&a, &b, &there, dir.path().join("inbox-b")),
                (&b, &a, &back, dir.path().join("inbox-a")),
            ] {
                let shared = sending(what.clone(), from);
                let waiting = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
                let receipt = take(&waiting, to, &inbox, Arc::default()).await.unwrap();
                assert_eq!(receipt.name, what.file_name().unwrap().to_str().unwrap());
                assert!(Path::new(&receipt.path).is_dir());
                wait_for(&shared, |s| matches!(s, CodeSend::Sent { .. })).await;
            }
            let got = dir.path().join("inbox-b").join("Holiday");
            for (inside, data) in &files {
                assert_eq!(&std::fs::read(got.join(inside)).unwrap(), data, "{inside}");
            }
            let got = dir.path().join("inbox-a").join("Reply/thanks.txt");
            assert_eq!(std::fs::read(got).unwrap(), b"thanks");
        });
    }

    /// Several files and a folder go as one transfer, each under its name.
    #[test]
    fn several_items_go_as_one_transfer() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (device(dir.path(), "a"), device(dir.path(), "b"));
        let one = dir.path().join("one.txt");
        std::fs::write(&one, b"1").unwrap();
        let album = dir.path().join("Album");
        let files = folder(&album);
        TOKIO.block_on(async {
            let shared = sending_all(vec![one, album], &a);
            let waiting = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
            let inbox = dir.path().join("inbox");
            let receipt = take(&waiting, &b, &inbox, Arc::default()).await.unwrap();
            assert_eq!(receipt.name, "2 items");
            let got = Path::new(&receipt.path);
            assert_eq!(std::fs::read(got.join("one.txt")).unwrap(), b"1");
            for (inside, data) in &files {
                assert_eq!(
                    &std::fs::read(got.join("Album").join(inside)).unwrap(),
                    data
                );
            }
        });
        // Two items with one name: refused, never one hiding the other.
        std::fs::create_dir_all(dir.path().join("other")).unwrap();
        std::fs::write(dir.path().join("other/one.txt"), b"2").unwrap();
        let clash = vec![dir.path().join("one.txt"), dir.path().join("other/one.txt")];
        assert!(items_in(&clash).is_err());
    }

    /// The receiver stops halfway and Relay closes (its node shuts down);
    /// after a relaunch the sender's next code resumes from what arrived,
    /// and the result verifies.
    #[test]
    fn a_transfer_stopped_halfway_resumes_after_a_relaunch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.bin");
        let content = bytes(24_000_000, 3);
        std::fs::write(&file, &content).unwrap();
        let a = device(dir.path(), "a");
        let shared = sending(file, &a);
        let inbox = dir.path().join("inbox");

        TOKIO.block_on(async {
            let first = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
            let b = device(dir.path(), "b");
            let progress = Arc::new(CodeProgress::default());
            progress.stop_at.store(4_000_000, Ordering::SeqCst);
            let failed = take(&first, &b, &inbox, progress).await.unwrap_err();
            assert_eq!(failed.offered.unwrap().size, 24_000_000);
            b.get().await.unwrap().shutdown().await;
            drop(b);

            let second = wait_for(&shared, |s| {
                matches!(s, CodeSend::Waiting { note: Some(_), .. })
            })
            .await;
            let b = device(dir.path(), "b");
            let receipt = take(&second, &b, &inbox, Arc::default()).await.unwrap();
            assert!(
                receipt.resumed_from >= 4_000_000,
                "resumed from {}",
                receipt.resumed_from
            );
            assert_eq!(std::fs::read(&receipt.path).unwrap(), content);
            wait_for(&shared, |s| matches!(s, CodeSend::Sent { .. })).await;
        });
    }

    /// Stopping the send stops a receiver that is fetching.
    #[test]
    fn stopping_the_sender_stops_the_receiver() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.bin");
        std::fs::write(&file, bytes(24_000_000, 4)).unwrap();
        let (a, b) = (device(dir.path(), "a"), device(dir.path(), "b"));
        let shared = sending(file, &a);
        TOKIO.block_on(async {
            let first = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
            let receiving = {
                let first = first.clone();
                let b = Arc::clone(&b);
                let inbox = dir.path().join("inbox");
                tokio::spawn(async move { take(&first, &b, &inbox, Arc::default()).await })
            };
            wait_for(&shared, |s| matches!(s, CodeSend::Sending { .. })).await;
            shared.stop.store(true, Ordering::SeqCst);
            let ended = tokio::time::timeout(Duration::from_secs(20), receiving)
                .await
                .expect("the receiver stopped in time")
                .unwrap();
            // A small file can finish before the stop lands; a stop is the
            // usual ending.
            if let Err(failed) = ended {
                assert!(failed.offered.is_some());
            }
            wait_for(&shared, |s| {
                matches!(s, CodeSend::Failed { .. } | CodeSend::Sent { .. })
            })
            .await;
        });
    }

    /// The app reads the status right after starting and stops polling if
    /// the send isn't running, so it must never read Idle there.
    #[test]
    fn a_send_is_running_as_soon_as_it_starts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        let shared = sending(file, &device(dir.path(), "a"));
        assert!(shared.running(), "{:?}", shared.status());
        shared.stop.store(true, Ordering::SeqCst);
        TOKIO.block_on(wait_for(&shared, |s| matches!(s, CodeSend::Failed { .. })));
    }

    #[test]
    fn an_empty_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(empty.join("inner")).unwrap();
        let shared = sending(empty, &device(dir.path(), "a"));
        let failed = TOKIO.block_on(wait_for(&shared, |s| matches!(s, CodeSend::Failed { .. })));
        assert!(matches!(failed, CodeSend::Failed { reason } if reason.contains("no files")));
    }

    #[test]
    fn a_clash_before_use_replaces_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        let shared = sending(file, &device(dir.path(), "a"));
        TOKIO.block_on(async {
            let first = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
            let CodeSend::Waiting { number, .. } = first else {
                unreachable!()
            };
            lock(&shared.nearby).insert(NearbySender {
                number,
                endpoint_id: "someone else".into(),
            });
            let next = wait_for(&shared, |s| {
                matches!(s, CodeSend::Waiting { note: Some(_), .. })
            })
            .await;
            let CodeSend::Waiting { number: n2, .. } = next else {
                unreachable!()
            };
            assert_ne!(n2, number);
            shared.stop.store(true, Ordering::SeqCst);
            wait_for(&shared, |s| matches!(s, CodeSend::Failed { .. })).await;
        });
    }

    #[test]
    fn folders_list_their_files_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("f");
        folder(&root);
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", root.join("outside")).unwrap();
        let names: Vec<String> = files_in(&root).unwrap().into_iter().map(|f| f.0).collect();
        assert_eq!(
            names,
            [
                "empty.bin",
                "notes.txt",
                "photos/a.jpg",
                "photos/deeper/b.raw"
            ]
        );
    }
}
