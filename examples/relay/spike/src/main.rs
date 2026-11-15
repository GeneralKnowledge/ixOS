//! Relay M0 spike: croc-style transfer with a word code on iroh.
//!
//! ```text
//! relay-spike send <file>             prints a code such as 7-acid-tidy-rug
//! relay-spike receive <code> [--addr ip:port --id <endpoint id>]
//! ```
//!
//! - iroh endpoints with relays turned off: local network only, no internet.
//! - The sender advertises `_forgerelay._udp` over mDNS (DNS-SD, the same
//!   records Bonjour reads) with the code's number and its endpoint id. It
//!   picks a number nobody nearby is using, and picks again if another
//!   sender shows up with the same number before the code is used.
//! - The receiver looks for that number. If two senders have it, it stops
//!   and asks for a new code instead of guessing, since a guess would spend
//!   a stranger's code.
//! - Both sides run SPAKE2 with the code's three words, then confirm the key
//!   with an HMAC over both endpoint ids (which QUIC already authenticated).
//! - One handshake at a time, each with a time limit; a second caller is told
//!   "busy" without spending the code. A wrong guess spends the code, as in
//!   magic-wormhole, and the sender issues a new one at once.
//! - Only then does the sender name the file (BLAKE3 hash, size, name) and
//!   let that endpoint id fetch it with iroh-blobs, which verifies every
//!   chunk. Both sides keep blobs on disk, so a dropped transfer resumes: the
//!   sender issues a new code and the receiver fetches only what's missing.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use hmac::{Hmac, Mac};
use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, TransportAddr};
use iroh_blobs::api::blobs::{AddPathOptions, ImportMode};
use iroh_blobs::provider::events::{
    AbortReason, ConnectMode, EventMask, EventSender, ProviderMessage,
};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{BlobFormat, BlobsProtocol, Hash, HashAndFormat};
use rand::Rng;
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use tokio::sync::mpsc;

mod session;
use session::{Answer, Ending};

const CODE_ALPN: &[u8] = b"forge-relay/code/1";
const SERVICE: &str = "_forgerelay._udp.local.";
const APP_ID: &str = "forge-relay/v1";
/// Time allowed for each step of the code handshake.
const STEP: Duration = Duration::from_secs(10);
/// How often a receiver says it is still there during a transfer, and how
/// long the sender waits before treating it as gone. Generous on purpose:
/// on a slow or bufferbloated path (a tailnet over a home line) ticks can
/// arrive seconds late, and giving up costs the receiver a new code. A
/// receiver that really vanished is still noticed in well under a minute,
/// and a resumed transfer keeps what already arrived.
const TICK: Duration = Duration::from_secs(2);
const SILENCE: Duration = Duration::from_secs(30);
/// How long a code stays valid.
const CODE_LIFETIME: Duration = Duration::from_secs(600);
/// New codes a sender issues (after wrong guesses, collisions or a dropped
/// transfer) before giving up.
const MAX_CODES: usize = 6;

// ---------------------------------------------------------------- the code

/// The EFF short word list 1: 1296 words of 3 to 5 letters, so three words
/// carry about 31 bits and are still short to type.
static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    include_str!("eff_short_wordlist_1.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect()
});

#[derive(Clone, Debug, PartialEq, Eq)]
struct Code {
    nameplate: u8,
    words: [String; 3],
}

impl Code {
    fn new(in_use: &HashSet<u8>) -> Self {
        let mut rng = rand::thread_rng();
        let free: Vec<u8> = (1..=99).filter(|n| !in_use.contains(n)).collect();
        // Test only: force a number, to provoke a collision.
        let forced = std::env::var("RELAY_SPIKE_NAMEPLATE")
            .ok()
            .and_then(|n| n.parse::<u8>().ok());
        let nameplate = if let Some(n) = forced {
            n
        } else if free.is_empty() {
            rng.gen_range(1..=99)
        } else {
            free[rng.gen_range(0..free.len())]
        };
        let pick =
            |rng: &mut rand::rngs::ThreadRng| WORDS[rng.gen_range(0..WORDS.len())].to_string();
        let words = [pick(&mut rng), pick(&mut rng), pick(&mut rng)];
        Code { nameplate, words }
    }

    fn parse(text: &str) -> Result<Self> {
        let parts: Vec<String> = text
            .trim()
            .to_lowercase()
            .split(|c: char| c == '-' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect();
        let [n, a, b, c] = parts.as_slice() else {
            bail!("a code is a number and three words, like 7-acid-tidy-rug");
        };
        let nameplate: u8 = n.parse().context("the code starts with a number")?;
        for w in [a, b, c] {
            if !WORDS.contains(&w.as_str()) {
                bail!("\"{w}\" isn't one of the code words; check the spelling");
            }
        }
        Ok(Code {
            nameplate,
            words: [a.clone(), b.clone(), c.clone()],
        })
    }

    fn text(&self) -> String {
        format!("{}-{}", self.nameplate, self.words.join("-"))
    }

    fn pake(&self) -> (Spake2<Ed25519Group>, Vec<u8>) {
        Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(self.words.join("-").as_bytes()),
            &Identity::new(format!("{APP_ID}/{}", self.nameplate).as_bytes()),
        )
    }
}

fn confirm(key: &[u8], role: &str, sender: EndpointId, receiver: EndpointId) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(role.as_bytes());
    mac.update(sender.as_bytes());
    mac.update(receiver.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

// ---------------------------------------------------------------- framing

/// The sender's first reply says how the attempt goes.
const GO: u8 = b'K';
const BUSY: u8 = b'B';
const GONE: u8 = b'G';

async fn put(send: &mut SendStream, bytes: &[u8]) -> Result<()> {
    send.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    send.write_all(bytes).await?;
    Ok(())
}

async fn get(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > 64 * 1024 {
        bail!("message too long");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn get_within(recv: &mut RecvStream, limit: Duration) -> Result<Vec<u8>> {
    tokio::time::timeout(limit, get(recv))
        .await
        .map_err(|_| anyhow!("timed out"))?
}

// ---------------------------------------------------------------- mDNS

/// Every sender advertising nearby: code number -> endpoint ids.
fn browse(window: Duration) -> Result<HashMap<u8, HashSet<EndpointId>>> {
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let events = mdns.browse(SERVICE)?;
    let deadline = Instant::now() + window;
    let mut seen: HashMap<u8, HashSet<EndpointId>> = HashMap::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            if let Some((n, id, _)) = read_service(&info) {
                seen.entry(n).or_default().insert(id);
            }
        }
    }
    mdns.shutdown().ok();
    Ok(seen)
}

fn read_service(info: &mdns_sd::ServiceInfo) -> Option<(u8, EndpointId, EndpointAddr)> {
    let n: u8 = info.get_property_val_str("n")?.parse().ok()?;
    let id: EndpointId = info.get_property_val_str("id")?.parse().ok()?;
    let port = info.get_port();
    let addrs = info
        .get_addresses()
        .iter()
        .map(|ip| TransportAddr::Ip(SocketAddr::new(*ip, port)))
        .collect::<Vec<_>>();
    Some((n, id, EndpointAddr::from_parts(id, addrs)))
}

struct Advert {
    mdns: mdns_sd::ServiceDaemon,
    fullname: Option<String>,
    id: EndpointId,
    port: u16,
}

impl Advert {
    fn new(id: EndpointId, port: u16) -> Result<Self> {
        Ok(Self {
            mdns: mdns_sd::ServiceDaemon::new()?,
            fullname: None,
            id,
            port,
        })
    }

    fn show(&mut self, nameplate: u8) -> Result<()> {
        if let Some(old) = self.fullname.take() {
            let _ = self.mdns.unregister(&old);
        }
        let short = &self.id.to_string()[..8];
        let host = format!("relay-{short}.local.");
        let txt = [
            ("v", "1".to_string()),
            ("n", nameplate.to_string()),
            ("id", self.id.to_string()),
        ];
        let info = mdns_sd::ServiceInfo::new(
            SERVICE,
            &format!("relay-{short}-{nameplate}"),
            &host,
            "",
            self.port,
            &txt[..],
        )?
        .enable_addr_auto();
        self.fullname = Some(info.get_fullname().to_string());
        self.mdns.register(info)?;
        Ok(())
    }
}

impl Drop for Advert {
    fn drop(&mut self) {
        if let Some(old) = self.fullname.take() {
            let _ = self.mdns.unregister(&old);
        }
        let _ = self.mdns.shutdown();
    }
}

// ---------------------------------------------------------------- sending

#[derive(Clone)]
struct Offer {
    hash: Hash,
    size: u64,
    name: String,
}

/// The live code and its state machine ([`session::Session`]).
struct Session {
    code: Code,
    issued: Instant,
    machine: session::Session,
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Done,
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
    session: Arc<Mutex<Session>>,
    offer: Offer,
    allowed: Arc<Mutex<HashSet<EndpointId>>>,
    events: mpsc::Sender<Event>,
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
    async fn run(&self, conn: &Connection) -> Result<Option<Event>> {
        let them = conn.remote_id();
        let (mut send, mut recv) = tokio::time::timeout(STEP, conn.accept_bi())
            .await
            .map_err(|_| anyhow!("no stream"))??;
        let their_msg = get_within(&mut recv, STEP).await?;
        let admitted = {
            let mut s = self.session.lock().unwrap();
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
        let accepted = {
            let mut s = self.session.lock().unwrap();
            s.machine.finish(gen, ending)
        };
        let kind = match ending {
            Ending::WrongCode => Kind::WrongCode,
            Ending::Dropped => Kind::Dropped,
            Ending::Proved if !accepted => Kind::Dropped,
            Ending::Proved => {
                self.allowed.lock().unwrap().insert(them);
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
        code: &Code,
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
        // While the file moves, the receiver ticks every couple of seconds,
        // then says "done" once it is verified and saved. Silence means it
        // went away (QUIC alone can take minutes to notice).
        loop {
            match get_within(recv, SILENCE).await {
                Ok(m) if m == b"tick" => continue,
                Ok(m) if m == b"done" => return Kind::Done,
                Ok(_) => return Kind::Dropped,
                Err(err) => {
                    eprintln!("receiver went quiet for {}s: {err}", SILENCE.as_secs());
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
            Err(err) => eprintln!("a connection failed: {err}"),
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
                    .map(|id| allowed.lock().unwrap().contains(&id))
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

/// Watches for another sender taking our number before our code is used.
fn watch_collisions(me: EndpointId, session: Arc<Mutex<Session>>, events: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let Ok(mdns) = mdns_sd::ServiceDaemon::new() else {
            return;
        };
        let Ok(rx) = mdns.browse(SERVICE) else { return };
        // One report per code: mDNS repeats itself, and a report about a
        // code that was already replaced is ignored by the sender.
        let mut reported: Option<u64> = None;
        while let Ok(event) = rx.recv() {
            if events.is_closed() {
                break;
            }
            if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
                let Some((n, id, _)) = read_service(&info) else {
                    continue;
                };
                let gen = {
                    let s = session.lock().unwrap();
                    (id != me && n == s.code.nameplate && !s.machine.spent).then_some(s.machine.gen)
                };
                if let Some(gen) = gen {
                    if reported != Some(gen) {
                        reported = Some(gen);
                        let _ = events.blocking_send(Event {
                            gen,
                            kind: Kind::Collision,
                        });
                    }
                }
            }
        }
        let _ = mdns.shutdown();
    });
}

async fn local_endpoint() -> Result<Endpoint> {
    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    Ok(endpoint)
}

async fn send(path: PathBuf, store_dir: &Path) -> Result<()> {
    let store = FsStore::load(store_dir).await?;
    let started = Instant::now();
    let tag = store
        .blobs()
        .add_path_with_opts(AddPathOptions {
            path: std::path::absolute(&path)?,
            format: BlobFormat::Raw,
            // Hash the file where it is instead of copying it into the store.
            mode: ImportMode::TryReference,
        })
        .await?;
    let size = std::fs::metadata(&path)?.len();
    eprintln!(
        "hashed {} bytes in {:.1}s",
        size,
        started.elapsed().as_secs_f64()
    );
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let endpoint = local_endpoint().await?;
    let allowed = Arc::new(Mutex::new(HashSet::new()));
    let blobs = BlobsProtocol::new(&store, Some(gate(allowed.clone())));
    let in_use: HashSet<u8> = browse(Duration::from_millis(1500))?.into_keys().collect();
    let session = Arc::new(Mutex::new(Session {
        code: Code::new(&in_use),
        issued: Instant::now(),
        machine: session::Session::new(),
    }));
    let (tx, mut events) = mpsc::channel(8);
    let handler = CodeHandler {
        me: endpoint.id(),
        session: session.clone(),
        offer: Offer {
            hash: tag.hash,
            size,
            name,
        },
        allowed,
        events: tx.clone(),
    };
    let router = Router::builder(endpoint.clone())
        .accept(CODE_ALPN, handler)
        .accept(iroh_blobs::ALPN, blobs)
        .spawn();

    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .map(|s| s.port())
        .ok_or_else(|| anyhow!("no IPv4 socket"))?;
    let mut advert = Advert::new(endpoint.id(), port)?;
    let first = session.lock().unwrap().code.clone();
    advert.show(first.nameplate)?;
    println!("Code: {}", first.text());
    eprintln!("hash {}", tag.hash);
    eprintln!(
        "(without mDNS: receive {} --addr 127.0.0.1:{port} --id {})",
        first.text(),
        endpoint.id()
    );
    watch_collisions(endpoint.id(), session.clone(), tx);

    let mut codes = 1;
    let result = loop {
        let left = CODE_LIFETIME.saturating_sub(session.lock().unwrap().issued.elapsed());
        let event = match tokio::time::timeout(left, events.recv()).await {
            Ok(Some(e)) => e,
            Ok(None) => break Err(anyhow!("stopped")),
            Err(_) => break Err(anyhow!("the code expired")),
        };
        let stale = {
            let mut s = session.lock().unwrap();
            match event.kind {
                Kind::Done => false,
                // Checked and acted on under one lock: a collision queued
                // while the code was unused may arrive after a receiver was
                // admitted, and must not cut that handshake off.
                Kind::Collision => !s.machine.collided(event.gen),
                _ => !s.machine.is_current(event.gen),
            }
        };
        if stale {
            // About a code that has been used or replaced since.
            continue;
        }
        let why = match event.kind {
            Kind::Done => break Ok(()),
            Kind::WrongCode => "Someone tried a wrong code, so it no longer works.",
            Kind::Dropped => "The transfer stopped before the end. To resume it,",
            Kind::Collision => "Another sender nearby took the same number.",
        };
        if codes >= MAX_CODES {
            break Err(anyhow!("{why} Too many new codes; stopping."));
        }
        codes += 1;
        let in_use: HashSet<u8> = browse(Duration::from_millis(1000))?
            .into_iter()
            .filter(|(_, ids)| !ids.contains(&endpoint.id()) || ids.len() > 1)
            .map(|(n, _)| n)
            .collect();
        let code = Code::new(&in_use);
        {
            let mut s = session.lock().unwrap();
            s.code = code.clone();
            s.issued = Instant::now();
            s.machine.reissue();
        }
        advert.show(code.nameplate)?;
        println!("{why} New code: {}", code.text());
    };
    drop(advert);
    router.shutdown().await?;
    store.shutdown().await.ok();
    result?;
    println!("Sent.");
    Ok(())
}

// ---------------------------------------------------------------- receiving

/// Finds the one sender advertising `nameplate`. Two of them is an error,
/// never a guess.
fn find(nameplate: u8) -> Result<EndpointAddr> {
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let events = mdns.browse(SERVICE)?;
    let mut deadline = Instant::now() + Duration::from_secs(10);
    let mut found: HashMap<EndpointId, EndpointAddr> = HashMap::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            if let Some((n, id, addr)) = read_service(&info) {
                if n == nameplate {
                    if found.is_empty() {
                        // Keep listening a little for a second sender.
                        deadline = Instant::now() + Duration::from_millis(1500);
                    }
                    found.insert(id, addr);
                }
            }
        }
    }
    mdns.shutdown().ok();
    match found.len() {
        0 => bail!("no sender with code number {nameplate} on this network"),
        1 => Ok(found.into_values().next().expect("one")),
        _ => {
            bail!("two senders nearby are using number {nameplate}; ask the sender for a new code")
        }
    }
}

async fn receive(
    code: &str,
    addr: Option<SocketAddr>,
    id: Option<EndpointId>,
    store_dir: &Path,
) -> Result<()> {
    let code = Code::parse(code)?;
    let target = match (addr, id) {
        (Some(addr), Some(id)) => EndpointAddr::from_parts(id, [TransportAddr::Ip(addr)]),
        _ => find(code.nameplate)?,
    };
    let sender = target.id;
    let endpoint = local_endpoint().await?;
    let me = endpoint.id();
    let conn = endpoint.connect(target.clone(), CODE_ALPN).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    let (state, my_msg) = code.pake();
    put(&mut send, &my_msg).await?;
    let first = get_within(&mut recv, STEP).await?;
    let refusal = match first.first() {
        Some(&GO) => None,
        Some(&BUSY) => Some("the sender is busy with someone else; try again in a moment"),
        Some(&GONE) => Some("that code has been used or has expired; ask for a new one"),
        _ => Some("unexpected answer"),
    };
    if let Some(reason) = refusal {
        conn.close(0u32.into(), b"ok");
        bail!(reason);
    }
    let key = state
        .finish(&first[1..])
        .map_err(|e| anyhow!("pake: {e:?}"))?;
    put(&mut send, &confirm(&key, "receiver", sender, me)).await?;
    let theirs = get_within(&mut recv, STEP)
        .await
        .map_err(|_| anyhow!("wrong code (the sender's code no longer works)"))?;
    if theirs != confirm(&key, "sender", sender, me) {
        bail!("the sender couldn't prove the code");
    }
    let offer = get_within(&mut recv, STEP).await?;
    if offer.len() < 40 {
        bail!("bad offer");
    }
    let hash = Hash::from_bytes(offer[..32].try_into()?);
    let size = u64::from_be_bytes(offer[32..40].try_into()?);
    let name = String::from_utf8_lossy(&offer[40..]).to_string();
    // A name, never a path.
    let name = Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let store = FsStore::load(store_dir).await?;
    let already = store
        .remote()
        .local(HashAndFormat::raw(hash))
        .await?
        .local_bytes();
    if already > 0 {
        eprintln!("resuming {name}: {already} of {size} bytes already here");
    } else {
        eprintln!("receiving {name} ({size} bytes, hash {hash})");
    }
    let started = Instant::now();
    let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stopped => break,
                _ = tokio::time::sleep(TICK) => {
                    if put(&mut send, b"tick").await.is_err() {
                        break;
                    }
                }
            }
        }
        send
    });
    let downloaded = store
        .downloader(&endpoint)
        .download(hash, Some(sender))
        .await;
    let secs = started.elapsed().as_secs_f64();
    // Keep ticking while the file is written out: a big export takes a while.
    let out = std::path::absolute(PathBuf::from("inbox").join(&name))?;
    let saved: Result<()> = match downloaded {
        Ok(()) => {
            std::fs::create_dir_all(out.parent().expect("inbox"))?;
            store
                .blobs()
                .export(hash, &out)
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
        }
        Err(err) => Err(anyhow!(
            "{err:#} after {:.1}s",
            started.elapsed().as_secs_f64()
        )),
    };
    let _ = stop.send(());
    let mut send = ticker.await?;
    saved?;
    put(&mut send, b"done").await?;
    send.finish()?;
    let fetched = size - already;
    println!(
        "Received {name}: {fetched} bytes fetched in {secs:.2}s ({:.0} MB/s), {size} in all, saved to {}",
        fetched as f64 / secs / 1e6,
        out.display()
    );
    conn.closed().await;
    endpoint.close().await;
    store.shutdown().await.ok();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let store_dir =
        PathBuf::from(flag("--store").unwrap_or_else(|| ".relay-spike-store".to_string()));
    match args.first().map(String::as_str) {
        Some("send") if args.len() >= 2 => send(PathBuf::from(&args[1]), &store_dir).await,
        Some("receive") if args.len() >= 2 => {
            let addr = flag("--addr").map(|a| a.parse()).transpose()?;
            let id = flag("--id").map(|a| a.parse()).transpose()?;
            receive(&args[1], addr, id, &store_dir).await
        }
        // Test only: answer with a malformed SPAKE2 message.
        Some("garble") => {
            let addr: SocketAddr = flag("--addr").context("--addr")?.parse()?;
            let id: EndpointId = flag("--id").context("--id")?.parse()?;
            let endpoint = local_endpoint().await?;
            let conn = endpoint
                .connect(
                    EndpointAddr::from_parts(id, [TransportAddr::Ip(addr)]),
                    CODE_ALPN,
                )
                .await?;
            let (mut send, mut recv) = conn.open_bi().await?;
            put(&mut send, b"not a spake2 message").await?;
            let first = get_within(&mut recv, STEP).await?;
            println!(
                "garble: sender answered {:?}",
                first.first().map(|b| *b as char)
            );
            conn.close(0u32.into(), b"bye");
            endpoint.close().await;
            Ok(())
        }
        // Test only: fetch a hash without the code, as a stranger would.
        Some("steal") if args.len() >= 2 => {
            let hex = &args[1];
            let mut raw = [0u8; 32];
            for (i, b) in raw.iter_mut().enumerate() {
                *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)?;
            }
            let hash = Hash::from_bytes(raw);
            let addr: SocketAddr = flag("--addr").context("--addr")?.parse()?;
            let id: EndpointId = flag("--id").context("--id")?.parse()?;
            let endpoint = local_endpoint().await?;
            // Connecting once teaches the endpoint the address.
            let _probe = endpoint
                .connect(
                    EndpointAddr::from_parts(id, [TransportAddr::Ip(addr)]),
                    iroh_blobs::ALPN,
                )
                .await;
            let store = FsStore::load(&store_dir).await?;
            let got = tokio::time::timeout(
                Duration::from_secs(15),
                store.downloader(&endpoint).download(hash, Some(id)),
            )
            .await;
            println!("stranger fetch: {got:?}");
            Ok(())
        }
        _ => {
            eprintln!(
                "relay-spike send <file> | receive <code> [--addr ip:port --id <id>] [--store dir]"
            );
            Ok(())
        }
    }
}
