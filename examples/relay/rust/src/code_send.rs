//! Sending a file with a word code (Relay M1): the iPhone as the sender, so
//! a computer (`relay-spike receive <code>`) or another phone can fetch from
//! it. The same protocol and rules as the M0 spike's sender:
//!
//! - One code is live at a time. A wrong guess, a dropped transfer or the
//!   code's lifetime running out replaces it with a new code; at most
//!   `MAX_CODES` are issued. The rules are `code_session::Session`, the
//!   state machine the Lean model (`verification/relay/CodeSession.lean`)
//!   proves: one guess per code, one receiver per code.
//! - Only an endpoint that proved the code may fetch the file
//!   (iroh-blobs' connection events gate every fetch).
//! - The app advertises the code with Bonjour (`_forgerelay._udp`, TXT
//!   `v=1`, `n` = the code's number, `id` = the endpoint id) on the port this
//!   returns, and re-advertises when the number changes. Rust can't use
//!   multicast on iOS, so it can't watch for another sender taking the same
//!   number; the app passes the numbers it saw in use, and new codes avoid
//!   them.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointId, RelayMode};
use iroh_blobs::api::blobs::{AddPathOptions, ImportMode};
use iroh_blobs::provider::events::{
    AbortReason, ConnectMode, EventMask, EventSender, ProviderMessage,
};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{BlobFormat, BlobsProtocol, Hash};
use tokio::sync::mpsc;

use crate::code::{
    confirm, connection, get_within, put, WordCode, BUSY, CODE_ALPN, GO, GONE, STEP, TOKIO,
};
use crate::code_session::{self, Answer, Ending};
use crate::model::RelayError;
use forge_runtime::lock;

/// How long the sender waits without a tick before treating the receiver
/// as gone (the receiver ticks every `code::TICK`).
const SILENCE: Duration = Duration::from_secs(30);
/// How long an unused code stays valid.
const CODE_LIFETIME: Duration = Duration::from_secs(600);
/// New codes issued (after wrong guesses or a dropped transfer) before
/// giving up.
const MAX_CODES: usize = 6;

/// Where a code send is.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CodeSendState {
    /// Waiting for a receiver to type the code.
    Waiting,
    /// A receiver proved the code and is fetching the file.
    Sending,
    /// The receiver has the file, verified and saved.
    Sent,
    /// Stopped: cancelled, too many new codes, or the code expired.
    Failed,
}

/// What the screen shows about a code send.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CodeSendStatus {
    pub state: CodeSendState,
    /// The live code, such as `37-grasp-gag-drum`.
    pub code: String,
    /// Its number, which Bonjour advertises.
    pub number: u8,
    /// Bumped each time a new code replaces the old one.
    pub generation: u64,
    /// Why there is a new code, or why the send stopped; empty otherwise.
    pub note: String,
    pub name: String,
    pub size: u64,
    /// From the receiver proving the code to it saying it has the file.
    pub seconds: f64,
    pub megabytes_per_second: f64,
}

/// A started code send: what the app advertises.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CodeOffer {
    pub endpoint_id: String,
    /// The endpoint's IPv4 UDP port.
    pub port: u16,
    pub status: CodeSendStatus,
}

/// The send in progress, shared with the app's polling.
#[derive(Default)]
pub struct CodeSending {
    pub status: Mutex<Option<CodeSendStatus>>,
    pub stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl CodeSending {
    fn update(&self, f: impl FnOnce(&mut CodeSendStatus)) {
        if let Some(s) = lock(&self.status).as_mut() {
            f(s);
        }
    }
}

#[derive(Clone)]
struct Offer {
    hash: Hash,
    size: u64,
    name: String,
}

struct Session {
    code: WordCode,
    issued: Instant,
    machine: code_session::Session,
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Done,
    WrongCode,
    Dropped,
}

#[derive(Debug)]
struct Event {
    gen: u64,
    kind: Kind,
}

#[derive(Clone)]
struct CodeHandler {
    me: EndpointId,
    session: Arc<Mutex<Session>>,
    offer: Offer,
    allowed: Arc<Mutex<HashSet<EndpointId>>>,
    events: mpsc::Sender<Event>,
    sending: Arc<CodeSending>,
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
                s.machine.lapse();
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
                lock(&self.allowed).insert(them);
                self.deliver(&mut send, &mut recv).await
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
        let mut offer = Vec::new();
        offer.extend_from_slice(self.offer.hash.as_bytes());
        offer.extend_from_slice(&self.offer.size.to_be_bytes());
        offer.extend_from_slice(self.offer.name.as_bytes());
        if put(send, &offer).await.is_err() {
            return Kind::Dropped;
        }
        let started = Instant::now();
        self.sending.update(|s| {
            s.state = CodeSendState::Sending;
            s.note.clear();
        });
        // The receiver ticks while the file moves, then says "done" once it
        // is verified and saved. Silence means it went away.
        loop {
            match get_within(recv, SILENCE).await {
                Ok(m) if m == b"tick" => continue,
                Ok(m) if m == b"done" => {
                    let seconds = started.elapsed().as_secs_f64();
                    self.sending.update(|s| {
                        s.seconds = seconds;
                        s.megabytes_per_second = if seconds > 0.0 {
                            s.size as f64 / seconds / 1e6
                        } else {
                            0.0
                        };
                    });
                    return Kind::Done;
                }
                Ok(_) => return Kind::Dropped,
                Err(_) => {
                    log::info!("code send: the receiver went quiet");
                    return Kind::Dropped;
                }
            }
        }
    }
}

impl ProtocolHandler for CodeHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
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
        Ok(())
    }
}

/// Lets only endpoints that proved a code fetch blobs.
fn gate(allowed: Arc<Mutex<HashSet<EndpointId>>>) -> EventSender {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        ..EventMask::DEFAULT
    };
    let (events, mut rx) = EventSender::channel(32, mask);
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let ProviderMessage::ClientConnected(msg) = msg {
                let ok = msg
                    .inner
                    .endpoint_id
                    .map(|id| lock(&allowed).contains(&id))
                    .unwrap_or(false);
                let answer = if ok {
                    Ok(())
                } else {
                    Err(AbortReason::Permission)
                };
                msg.tx.send(answer).await.ok();
            }
        }
    });
    events
}

/// Starts sending the file at `path`: hashes it into `store`, opens an
/// endpoint, and returns the first code and where to advertise it. The send
/// then runs on its own until it is done, fails or is stopped through
/// `sending.stop`; `sending.status` follows it.
pub async fn start(
    path: &Path,
    numbers_in_use: Vec<u8>,
    store: &Path,
    sending: Arc<CodeSending>,
) -> Result<CodeOffer, RelayError> {
    let path = path.to_path_buf();
    let store = store.to_path_buf();
    // iroh needs tokio; the caller (UniFFI's executor) may not be tokio.
    TOKIO
        .spawn(async move { begin(path, numbers_in_use, store, sending).await })
        .await
        .unwrap_or_else(|e| Err(connection(e)))
}

async fn begin(
    path: PathBuf,
    numbers_in_use: Vec<u8>,
    store_dir: PathBuf,
    sending: Arc<CodeSending>,
) -> Result<CodeOffer, RelayError> {
    let file = |e: &dyn std::fmt::Display| RelayError::File {
        reason: e.to_string(),
    };
    let size = std::fs::metadata(&path).map_err(|e| file(&e))?.len();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let store = FsStore::load(&store_dir).await.map_err(|e| file(&e))?;
    let tag = store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: path.clone(),
            format: BlobFormat::Raw,
            // Hash the file where it is instead of copying it into the store.
            mode: ImportMode::TryReference,
        })
        .await
        .map_err(|e| file(&e))?;

    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .map_err(|e| RelayError::Unreachable {
            reason: e.to_string(),
        })?;
    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .map(|s| s.port())
        .ok_or_else(|| RelayError::Unreachable {
            reason: "no IPv4 socket".into(),
        })?;
    let allowed = Arc::new(Mutex::new(HashSet::new()));
    let blobs = BlobsProtocol::new(&store, Some(gate(Arc::clone(&allowed))));
    let in_use: Vec<u8> = numbers_in_use;
    let first = WordCode::random(&in_use);
    let session = Arc::new(Mutex::new(Session {
        code: first.clone(),
        issued: Instant::now(),
        machine: code_session::Session::new(),
    }));
    let status = CodeSendStatus {
        state: CodeSendState::Waiting,
        code: first.text(),
        number: first.number,
        generation: 0,
        note: String::new(),
        name: name.clone(),
        size,
        seconds: 0.0,
        megabytes_per_second: 0.0,
    };
    *lock(&sending.status) = Some(status.clone());
    let stop = Arc::new(AtomicBool::new(false));
    *lock(&sending.stop) = Some(Arc::clone(&stop));

    let (tx, events) = mpsc::channel(8);
    let handler = CodeHandler {
        me: endpoint.id(),
        session: Arc::clone(&session),
        offer: Offer {
            hash: tag.hash,
            size,
            name,
        },
        allowed,
        events: tx,
        sending: Arc::clone(&sending),
    };
    let router = Router::builder(endpoint.clone())
        .accept(CODE_ALPN, handler)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
    let offer = CodeOffer {
        endpoint_id: endpoint.id().to_string(),
        port,
        status,
    };
    log::info!("code send: {size} bytes, waiting for a receiver");
    tokio::spawn(async move {
        let result = serve(&session, events, &sending, &stop, &in_use).await;
        let _ = router.shutdown().await;
        store.shutdown().await.ok();
        sending.update(|s| match result {
            Ok(()) => {
                s.state = CodeSendState::Sent;
                s.note.clear();
            }
            Err(why) => {
                s.state = CodeSendState::Failed;
                s.note = why;
            }
        });
        *lock(&sending.stop) = None;
    });
    Ok(offer)
}

/// The sender's loop: waits for the transfer to finish, replacing the code
/// after a wrong guess or a dropped transfer.
async fn serve(
    session: &Mutex<Session>,
    mut events: mpsc::Receiver<Event>,
    sending: &CodeSending,
    stop: &AtomicBool,
    in_use: &[u8],
) -> Result<(), String> {
    let mut codes = 1;
    loop {
        // The lifetime limits how long an unused code can be redeemed. Once
        // a receiver is in, the transfer may take as long as it needs.
        let left = {
            let s = lock(session);
            (!s.machine.spent).then(|| CODE_LIFETIME.saturating_sub(s.issued.elapsed()))
        };
        let wait = async {
            match left {
                Some(left) => tokio::time::timeout(left, events.recv()).await,
                None => Ok(events.recv().await),
            }
        };
        let stopped = async {
            while !stop.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };
        let next = tokio::select! {
            n = wait => n,
            _ = stopped => return Err("Stopped.".into()),
        };
        let event = match next {
            Ok(Some(e)) => e,
            Ok(None) => return Err("Stopped.".into()),
            Err(_) => {
                // A receiver may have been admitted just as time ran out:
                // only an unused code lapses.
                if lock(session).machine.lapse() {
                    return Err("The code expired. Start again for a new one.".into());
                }
                continue;
            }
        };
        if event.kind != Kind::Done && !lock(session).machine.is_current(event.gen) {
            continue;
        }
        let why = match event.kind {
            Kind::Done => return Ok(()),
            Kind::WrongCode => "Someone tried a wrong code, so here is a new one.",
            Kind::Dropped => "The transfer stopped before the end. To resume it, use the new code.",
        };
        if codes >= MAX_CODES {
            return Err(format!("{} Too many new codes; stopped.", why));
        }
        codes += 1;
        // Avoid the numbers other senders were using, and the old one.
        let mut avoid = in_use.to_vec();
        let old = lock(session).code.number;
        avoid.push(old);
        let code = WordCode::random(&avoid);
        let gen = {
            let mut s = lock(session);
            s.code = code.clone();
            s.issued = Instant::now();
            s.machine.reissue();
            s.machine.gen
        };
        log::info!("code send: new code (generation {gen})");
        sending.update(|s| {
            s.state = CodeSendState::Waiting;
            s.code = code.text();
            s.number = code.number;
            s.generation = gen;
            s.note = why.to_string();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::{receive, CodeProgress};

    fn wait_for(sending: &CodeSending, done: impl Fn(&CodeSendStatus) -> bool) -> CodeSendStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(s) = lock(&sending.status).clone() {
                if done(&s) {
                    return s;
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out: {:?}",
                lock(&sending.status)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Bytes that differ from offset to offset, so a misplaced chunk shows.
    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn fetch(code: &str, offer: &CodeOffer, dir: &Path) -> Result<crate::CodeReceipt, RelayError> {
        TOKIO.block_on(receive(
            code,
            &[format!("127.0.0.1:{}", offer.port)],
            &offer.endpoint_id,
            &dir.join("store"),
            &dir.join("inbox"),
            Arc::new(CodeProgress::default()),
            Arc::new(AtomicBool::new(false)),
        ))
    }

    #[test]
    fn the_receiver_that_types_the_code_gets_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("photo.bin");
        let bytes = data(3_000_000);
        std::fs::write(&file, &bytes).unwrap();
        let sending = Arc::new(CodeSending::default());
        let offer = TOKIO
            .block_on(start(
                &file,
                vec![],
                &dir.path().join("send"),
                Arc::clone(&sending),
            ))
            .unwrap();
        assert_eq!(offer.status.state, CodeSendState::Waiting);
        assert_eq!(offer.status.size, bytes.len() as u64);

        let receipt = fetch(&offer.status.code, &offer, &dir.path().join("recv")).unwrap();
        assert_eq!(receipt.name, "photo.bin");
        assert_eq!(std::fs::read(&receipt.path).unwrap(), bytes);
        let done = wait_for(&sending, |s| {
            s.state != CodeSendState::Waiting && s.state != CodeSendState::Sending
        });
        assert_eq!(done.state, CodeSendState::Sent, "{done:?}");
        assert!(
            lock(&sending.stop).is_none(),
            "a finished send releases its slot"
        );
    }

    #[test]
    fn a_wrong_code_brings_a_new_code_that_works() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"hello from the phone").unwrap();
        let sending = Arc::new(CodeSending::default());
        let offer = TOKIO
            .block_on(start(
                &file,
                vec![],
                &dir.path().join("send"),
                Arc::clone(&sending),
            ))
            .unwrap();
        // Same number, other words.
        let words = |c: &str| c.split_once('-').unwrap().1.to_string();
        let wrong = loop {
            let other = WordCode::random(&[]);
            if words(&other.text()) != words(&offer.status.code) {
                break format!("{}-{}", offer.status.number, words(&other.text()));
            }
        };
        let err = fetch(&wrong, &offer, &dir.path().join("recv")).unwrap_err();
        assert!(matches!(err, RelayError::Declined { .. }), "{err:?}");
        let renewed = wait_for(&sending, |s| s.generation == 1);
        assert_eq!(renewed.state, CodeSendState::Waiting);
        assert!(!renewed.note.is_empty());
        assert_ne!(
            renewed.number, offer.status.number,
            "a new code gets a new number"
        );
        // (Trying the old code now would count as a guess against the new
        // one and spend it too: one guess per live code.)
        let receipt = fetch(&renewed.code, &offer, &dir.path().join("recv")).unwrap();
        assert_eq!(
            std::fs::read(&receipt.path).unwrap(),
            b"hello from the phone"
        );
        assert_eq!(
            wait_for(&sending, |s| s.state == CodeSendState::Sent).generation,
            1
        );
    }

    #[test]
    fn stopping_ends_the_send() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        std::fs::write(&file, b"x").unwrap();
        let sending = Arc::new(CodeSending::default());
        TOKIO
            .block_on(start(
                &file,
                vec![],
                &dir.path().join("send"),
                Arc::clone(&sending),
            ))
            .unwrap();
        lock(&sending.stop)
            .as_ref()
            .unwrap()
            .store(true, Ordering::SeqCst);
        let s = wait_for(&sending, |s| s.state == CodeSendState::Failed);
        assert_eq!(s.note, "Stopped.");
    }
}
