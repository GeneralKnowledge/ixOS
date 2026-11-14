//! Relay stage 0 spike: croc-style transfer with a word code on iroh.
//!
//! ```text
//! relay-spike send <file>             prints a code such as 7-orbit-velvet
//! relay-spike receive <code> [--addr ip:port --id <endpoint id>]
//! ```
//!
//! - iroh endpoints with relays turned off: local network only, no internet.
//! - The sender advertises `_forgerelay._udp` over mDNS (DNS-SD, the same
//!   records Bonjour reads) with the code's number and its endpoint id.
//! - The receiver connects on the code protocol and both sides run SPAKE2
//!   with the code's words, then confirm the key with an HMAC over both
//!   endpoint ids (which QUIC has already authenticated).
//! - Only then does the sender tell the receiver the file's BLAKE3 hash and
//!   allow that endpoint id to fetch it with iroh-blobs, which verifies
//!   every chunk against the hash while streaming.
//! - One wrong guess ends the code, as in magic-wormhole.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use hmac::{Hmac, Mac};
use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, TransportAddr};
use iroh_blobs::provider::events::{
    AbortReason, ConnectMode, EventMask, EventSender, ProviderMessage,
};
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::{BlobsProtocol, Hash};
use rand::Rng;
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use tokio::sync::mpsc;

const CODE_ALPN: &[u8] = b"forge-relay/code/0";
const SERVICE: &str = "_forgerelay._udp.local.";
const APP_ID: &str = "forge-relay/v1";

// ---------------------------------------------------------------- the code

struct Code {
    nameplate: u8,
    words: String,
}

fn new_code() -> Code {
    let mut rng = rand::thread_rng();
    let nameplate = rng.gen_range(1..=99u8);
    let bytes: [u8; 2] = rng.gen();
    // PGP words alternate between an even list and an odd list, so a
    // swapped pair of words is caught as a typo.
    let words = pgp_words::to_words(&bytes).join("-").to_lowercase();
    Code { nameplate, words }
}

fn parse_code(code: &str) -> Result<Code> {
    let (n, words) = code
        .split_once('-')
        .ok_or_else(|| anyhow!("a code looks like 7-orbit-velvet"))?;
    let nameplate: u8 = n.parse().context("the code starts with a number")?;
    let list: Vec<&str> = words.split('-').collect();
    if list.len() != 2 || pgp_words::to_bytes(&list).is_none() {
        bail!("those aren't two code words in the right order");
    }
    Ok(Code {
        nameplate,
        words: words.to_lowercase(),
    })
}

fn pake(code: &Code) -> (spake2::Spake2<Ed25519Group>, Vec<u8>) {
    Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.words.as_bytes()),
        &Identity::new(format!("{APP_ID}/{}", code.nameplate).as_bytes()),
    )
}

fn confirm(key: &[u8], role: &str, sender: EndpointId, receiver: EndpointId) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(role.as_bytes());
    mac.update(sender.as_bytes());
    mac.update(receiver.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

// ---------------------------------------------------------------- framing

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

// ---------------------------------------------------------------- sending

#[derive(Clone)]
struct Offer {
    hash: Hash,
    size: u64,
    name: String,
}

#[derive(Clone)]
struct CodeHandler {
    me: EndpointId,
    code: Arc<Code>,
    offer: Offer,
    allowed: Arc<Mutex<HashSet<EndpointId>>>,
    /// The code is single use: the first attempt, right or wrong, spends it.
    spent: Arc<Mutex<bool>>,
    outcome: mpsc::Sender<Result<EndpointId, String>>,
}

impl std::fmt::Debug for CodeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CodeHandler")
    }
}

impl CodeHandler {
    async fn run(&self, conn: &Connection) -> Result<EndpointId> {
        let them = conn.remote_id();
        let me = self.me;
        let (mut send, mut recv) = conn.accept_bi().await?;
        let their_msg = get(&mut recv).await?;
        {
            let mut spent = self.spent.lock().unwrap();
            if *spent {
                bail!("code already used");
            }
            *spent = true;
        }
        let (state, my_msg) = pake(&self.code);
        put(&mut send, &my_msg).await?;
        let key = state
            .finish(&their_msg)
            .map_err(|e| anyhow!("pake: {e:?}"))?;
        let theirs = get(&mut recv).await?;
        if theirs != confirm(&key, "receiver", me, them) {
            bail!("someone tried a wrong code; it no longer works");
        }
        put(&mut send, &confirm(&key, "sender", me, them)).await?;
        self.allowed.lock().unwrap().insert(them);
        let mut offer = Vec::new();
        offer.extend_from_slice(self.offer.hash.as_bytes());
        offer.extend_from_slice(&self.offer.size.to_be_bytes());
        offer.extend_from_slice(self.offer.name.as_bytes());
        put(&mut send, &offer).await?;
        // Wait for "done" (or the connection closing).
        let _ = get(&mut recv).await;
        Ok(them)
    }
}

impl ProtocolHandler for CodeHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let result = self.run(&connection).await.map_err(|e| e.to_string());
        let _ = self.outcome.send(result).await;
        connection.closed().await;
        Ok(())
    }
}

/// Lets only endpoints that proved the code fetch blobs.
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

async fn local_endpoint() -> Result<Endpoint> {
    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    Ok(endpoint)
}

async fn send(path: PathBuf) -> Result<()> {
    let store = MemStore::new();
    let started = Instant::now();
    let tag = store.blobs().add_path(std::path::absolute(&path)?).await?;
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
    let code = Arc::new(new_code());
    let (tx, mut outcome) = mpsc::channel(4);
    let handler = CodeHandler {
        me: endpoint.id(),
        code: code.clone(),
        offer: Offer {
            hash: tag.hash,
            size,
            name,
        },
        allowed,
        spent: Arc::new(Mutex::new(false)),
        outcome: tx,
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
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let host = format!("relay-{}.local.", &endpoint.id().to_string()[..8]);
    let txt = [
        ("v", "1".to_string()),
        ("n", code.nameplate.to_string()),
        ("id", endpoint.id().to_string()),
    ];
    let info = mdns_sd::ServiceInfo::new(
        SERVICE,
        &format!("relay-{}", code.nameplate),
        &host,
        "",
        port,
        &txt[..],
    )?
    .enable_addr_auto();
    mdns.register(info)?;

    let code_text = format!("{}-{}", code.nameplate, code.words);
    println!("Code: {code_text}");
    eprintln!("hash {}", tag.hash);
    eprintln!(
        "(without mDNS: receive {code_text} --addr 127.0.0.1:{port} --id {})",
        endpoint.id()
    );

    let result = tokio::time::timeout(Duration::from_secs(600), outcome.recv())
        .await
        .map_err(|_| anyhow!("the code expired"))?
        .ok_or_else(|| anyhow!("stopped"))?;
    mdns.shutdown().ok();
    router.shutdown().await?;
    match result {
        Ok(_) => {
            println!("Sent.");
            Ok(())
        }
        Err(reason) => bail!(reason),
    }
}

// ---------------------------------------------------------------- receiving

async fn find(nameplate: u8) -> Result<EndpointAddr> {
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let events = mdns.browse(SERVICE)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            if info.get_property_val_str("n") != Some(&nameplate.to_string()) {
                continue;
            }
            let id: EndpointId = info
                .get_property_val_str("id")
                .ok_or_else(|| anyhow!("no id"))?
                .parse()?;
            let port = info.get_port();
            let addrs = info
                .get_addresses()
                .iter()
                .map(|ip| TransportAddr::Ip(SocketAddr::new(*ip, port)))
                .collect::<Vec<_>>();
            mdns.shutdown().ok();
            return Ok(EndpointAddr::from_parts(id, addrs));
        }
    }
    mdns.shutdown().ok();
    bail!("no sender with code number {nameplate} on this network")
}

async fn receive(code: &str, addr: Option<SocketAddr>, id: Option<EndpointId>) -> Result<()> {
    let code = parse_code(code)?;
    let target = match (addr, id) {
        (Some(addr), Some(id)) => EndpointAddr::from_parts(id, [TransportAddr::Ip(addr)]),
        _ => find(code.nameplate).await?,
    };
    let sender = target.id;
    let endpoint = local_endpoint().await?;
    let me = endpoint.id();
    let conn = endpoint.connect(target.clone(), CODE_ALPN).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    let (state, my_msg) = pake(&code);
    put(&mut send, &my_msg).await?;
    let their_msg = get(&mut recv).await?;
    let key = state
        .finish(&their_msg)
        .map_err(|e| anyhow!("pake: {e:?}"))?;
    put(&mut send, &confirm(&key, "receiver", sender, me)).await?;
    let theirs = get(&mut recv)
        .await
        .map_err(|_| anyhow!("wrong code (the sender's code no longer works)"))?;
    if theirs != confirm(&key, "sender", sender, me) {
        bail!("the sender couldn't prove the code");
    }
    let offer = get(&mut recv).await?;
    if offer.len() < 40 {
        bail!("bad offer");
    }
    let hash = Hash::from_bytes(offer[..32].try_into()?);
    let size = u64::from_be_bytes(offer[32..40].try_into()?);
    let name = String::from_utf8_lossy(&offer[40..]).to_string();
    // A name, never a path.
    let name = std::path::Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    eprintln!("receiving {name} ({size} bytes, hash {hash})");

    let store = MemStore::new();
    let started = Instant::now();
    store
        .downloader(&endpoint)
        .download(hash, Some(sender))
        .await?;
    let secs = started.elapsed().as_secs_f64();
    let out = std::path::absolute(PathBuf::from("inbox").join(&name))?;
    std::fs::create_dir_all(out.parent().unwrap())?;
    store.blobs().export(hash, &out).await?;
    put(&mut send, b"done").await?;
    send.finish()?;
    println!(
        "Received {name}: {size} bytes in {secs:.2}s ({:.0} MB/s), saved to {}",
        size as f64 / secs / 1e6,
        out.display()
    );
    conn.close(0u32.into(), b"thanks");
    endpoint.close().await;
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
    match args.first().map(String::as_str) {
        Some("send") if args.len() >= 2 => send(PathBuf::from(&args[1])).await,
        Some("receive") if args.len() >= 2 => {
            let addr = flag("--addr").map(|a| a.parse()).transpose()?;
            let id = flag("--id").map(|a| a.parse()).transpose()?;
            receive(&args[1], addr, id).await
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
            let store = MemStore::new();
            let got = tokio::time::timeout(
                Duration::from_secs(15),
                store.downloader(&endpoint).download(hash, Some(id)),
            )
            .await;
            println!("stranger fetch: {got:?}");
            Ok(())
        }
        _ => {
            eprintln!("relay-spike send <file> | receive <code> [--addr ip:port --id <id>]");
            Ok(())
        }
    }
}
