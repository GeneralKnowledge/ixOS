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
use crate::code_session::{Answer, Ending, Session as Machine};
use crate::model::RelayError;

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

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts sending `path` with a word code in the background; follow it in
/// `shared.status`. Blobs are kept in `store` so a later send of the same
/// file need not hash it again.
pub fn start(path: PathBuf, store: PathBuf, shared: Arc<CodeSendShared>) {
    shared.stop.store(false, Ordering::SeqCst);
    TOKIO.spawn(async move {
        if let Err(err) = send(&path, &store, &shared).await {
            log::info!("code send ended: {err}");
            shared.set(CodeSend::Failed {
                reason: err.to_string(),
            });
        }
    });
}

#[derive(Clone)]
struct Offer {
    hash: Hash,
    size: u64,
    name: String,
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

#[derive(Clone)]
struct CodeHandler {
    me: EndpointId,
    port: u16,
    session: Arc<Mutex<Session>>,
    offer: Offer,
    allowed: Arc<Mutex<HashSet<EndpointId>>>,
    events: mpsc::Sender<Event>,
    shared: Arc<CodeSendShared>,
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
                lock(&self.allowed).insert(them);
                self.shared.set(CodeSend::Sending {
                    code: code.text(),
                    number: code.number,
                    port: self.port,
                    endpoint_id: self.me.to_string(),
                    name: self.offer.name.clone(),
                    size: self.offer.size,
                });
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
        let started = Instant::now();
        let mut offer = Vec::new();
        offer.extend_from_slice(self.offer.hash.as_bytes());
        offer.extend_from_slice(&self.offer.size.to_be_bytes());
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

fn file_error(e: impl std::fmt::Display) -> RelayError {
    RelayError::File {
        reason: e.to_string(),
    }
}

async fn send(
    path: &Path,
    store_dir: &Path,
    shared: &Arc<CodeSendShared>,
) -> Result<(), RelayError> {
    let size = std::fs::metadata(path).map_err(file_error)?.len();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    shared.set(CodeSend::Preparing {
        name: name.clone(),
        size,
    });
    let store = FsStore::load(store_dir).await.map_err(file_error)?;
    let outcome = serve(path, &store, name, size, shared).await;
    store.shutdown().await.ok();
    outcome
}

async fn serve(
    path: &Path,
    store: &FsStore,
    name: String,
    size: u64,
    shared: &Arc<CodeSendShared>,
) -> Result<(), RelayError> {
    let started = Instant::now();
    let tag = store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: std::path::absolute(path).map_err(file_error)?,
            format: BlobFormat::Raw,
            // Hash the file where it is instead of copying it into the store.
            mode: ImportMode::TryReference,
        })
        .await
        .map_err(file_error)?;
    log::info!(
        "code send: hashed {size} bytes in {:.1}s",
        started.elapsed().as_secs_f64()
    );

    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .map_err(|e| RelayError::Unreachable {
            reason: e.to_string(),
        })?;
    let me = endpoint.id();
    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .map(|s| s.port())
        .ok_or_else(|| RelayError::Unreachable {
            reason: "no IPv4 socket".into(),
        })?;
    let allowed = Arc::new(Mutex::new(HashSet::new()));
    let blobs = BlobsProtocol::new(store, Some(gate(Arc::clone(&allowed))));
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
        offer: Offer {
            hash: tag.hash,
            size,
            name: name.clone(),
        },
        allowed,
        events: tx,
        shared: Arc::clone(shared),
    };
    let router = Router::builder(endpoint.clone())
        .accept(CODE_ALPN, handler)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();
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
    router.shutdown().await.ok();
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::code::{receive, CodeProgress};

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

    /// The whole flow on one machine: a wrong code is spent and replaced,
    /// the new code delivers the file byte for byte.
    #[test]
    fn send_and_receive_with_a_code_over_loopback() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.bin");
        let bytes: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(&file, &bytes).unwrap();
        let shared = Arc::new(CodeSendShared::default());
        start(file, dir.path().join("send-store"), Arc::clone(&shared));

        TOKIO.block_on(async {
            let first = wait_for(&shared, |s| matches!(s, CodeSend::Waiting { .. })).await;
            let (code, addr, id) = address(&first);
            let number: u8 = code.split('-').next().unwrap().parse().unwrap();
            let wrong = WordCode::new(&[]).text();
            let wrong = format!("{number}{}", &wrong[wrong.find('-').unwrap()..]);
            let progress = Arc::new(CodeProgress::default());
            let refused = receive(
                &wrong,
                std::slice::from_ref(&addr),
                &id,
                &dir.path().join("recv-store"),
                &dir.path().join("inbox"),
                Arc::clone(&progress),
                Arc::new(AtomicBool::new(false)),
            )
            .await;
            assert!(refused.is_err(), "a wrong code must be refused");

            let second = wait_for(&shared, |s| {
                matches!(s, CodeSend::Waiting { note: Some(_), .. })
            })
            .await;
            let (code, addr, id) = address(&second);
            let receipt = receive(
                &code,
                &[addr],
                &id,
                &dir.path().join("recv-store"),
                &dir.path().join("inbox"),
                progress,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
            assert_eq!(std::fs::read(&receipt.path).unwrap(), bytes);
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

    #[test]
    fn a_clash_before_use_replaces_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        let shared = Arc::new(CodeSendShared::default());
        start(file, dir.path().join("store"), Arc::clone(&shared));
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
}
