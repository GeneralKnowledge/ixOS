//! Receiving a file or a folder with a word code, the transfer Relay is
//! moving to (examples/relay/docs/relay-platform-plan.md, M1). The sender is
//! another Relay (`code_send`) or the M0 spike (`relay-spike send <file>`):
//!
//! - The device's own iroh endpoint ([`crate::node`]), relays off: the local
//!   network only.
//! - The app finds the sender with Bonjour (`_forgerelay._udp`, its TXT
//!   record carries the code's number and the sender's endpoint id) and hands
//!   the addresses here; raw multicast needs an entitlement a free team
//!   can't have, so Rust does not browse on iOS.
//! - SPAKE2 with the code's three words, then a key confirmation over both
//!   endpoint ids. A wrong code is spent on the sender's side.
//! - Then iroh-blobs fetches the file, or a folder's files as a collection,
//!   verifying every chunk, into the device's store on disk, so receiving
//!   again with the sender's next code resumes, also after a relaunch.
//!
//! A file's wire protocol matches the spike's exactly; a folder's offer adds
//! a format byte. The sending side, which the iPhone uses too, is
//! `code_send`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{EndpointAddr, EndpointId, TransportAddr};
use iroh_blobs::api::downloader::DownloadProgressItem;
use iroh_blobs::format::collection::Collection;
use iroh_blobs::{Hash, HashAndFormat};
use n0_future::StreamExt;
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::model::RelayError;
use crate::node::{file_error, Node};

/// The Bonjour service a sender advertises.
pub const CODE_SERVICE_TYPE: &str = "_forgerelay._udp";
pub(crate) const CODE_ALPN: &[u8] = b"forge-relay/code/1";
const APP_ID: &str = "forge-relay/v1";
/// Time allowed for each step of the code handshake.
pub(crate) const STEP: Duration = Duration::from_secs(10);
/// How often the receiver tells the sender it is still there.
const TICK: Duration = Duration::from_secs(2);
pub(crate) const GO: u8 = b'K';
pub(crate) const BUSY: u8 = b'B';
pub(crate) const GONE: u8 = b'G';
/// The format byte of a folder's offer: an iroh-blobs collection.
pub(crate) const COLLECTION: u8 = 1;

/// The EFF short word list 1 (CC BY 3.0 US), as the sender uses.
static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    include_str!("eff_short_wordlist_1.txt")
        .lines()
        // "yo-yo" is left out: a dash separates a code's words.
        .filter(|l| !l.starts_with('#') && !l.is_empty() && !l.contains('-'))
        .collect()
});

/// One tokio runtime for iroh, started on first use.
pub(crate) static TOKIO: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("relay-code")
        .enable_all()
        .build()
        .expect("a tokio runtime")
});

/// A word code: a number to find the sender by, and three words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordCode {
    pub number: u8,
    words: [String; 3],
}

impl WordCode {
    /// A new code whose number nobody nearby is using (`in_use`), when one
    /// is free, and three random list words.
    pub fn new(in_use: &[u8]) -> Self {
        let random = forge_devproto::new_secret();
        let pick = |i: usize| {
            let n = u16::from_be_bytes([random[i], random[i + 1]]) as usize;
            n % WORDS.len()
        };
        let free: Vec<u8> = (1..=99).filter(|n| !in_use.contains(n)).collect();
        let number = if free.is_empty() {
            1 + (random[0] % 99)
        } else {
            free[pick(1) % free.len()]
        };
        let words = [3, 5, 7].map(|i| WORDS[pick(i)].to_string());
        Self { number, words }
    }

    /// `7-acid-tidy-rug`.
    pub fn text(&self) -> String {
        format!("{}-{}", self.number, self.words.join("-"))
    }

    pub fn parse(text: &str) -> Result<Self, RelayError> {
        let bad = |reason: &str| RelayError::WordCode {
            reason: reason.to_string(),
        };
        let parts: Vec<String> = text
            .trim()
            .to_lowercase()
            .split(|c: char| c == '-' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect();
        let [n, a, b, c] = parts.as_slice() else {
            return Err(bad(
                "a code is a number and three words, like 7-acid-tidy-rug",
            ));
        };
        let number: u8 = n
            .parse()
            .ok()
            .filter(|n| (1..=99).contains(n))
            .ok_or_else(|| bad("the code starts with a number from 1 to 99"))?;
        for w in [a, b, c] {
            if !WORDS.contains(&w.as_str()) {
                return Err(RelayError::WordCode {
                    reason: format!("\"{w}\" isn't one of the code words; check the spelling"),
                });
            }
        }
        Ok(Self {
            number,
            words: [a.clone(), b.clone(), c.clone()],
        })
    }

    pub(crate) fn pake(&self) -> (Spake2<Ed25519Group>, Vec<u8>) {
        Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(self.words.join("-").as_bytes()),
            &Identity::new(format!("{APP_ID}/{}", self.number).as_bytes()),
        )
    }
}

pub(crate) fn confirm(key: &[u8], role: &str, sender: EndpointId, receiver: EndpointId) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(role.as_bytes());
    mac.update(sender.as_bytes());
    mac.update(receiver.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// How far the current receive has got.
#[derive(Default)]
pub struct CodeProgress {
    pub active: AtomicBool,
    pub total: AtomicU64,
    pub already: AtomicU64,
    /// Bytes here so far, counting what was already here.
    pub done: AtomicU64,
    /// Tests: stop once this many bytes are here (0: never), to interrupt
    /// a transfer halfway on purpose.
    #[cfg(test)]
    pub stop_at: AtomicU64,
}

/// What the receiver learnt before a receive ended: the sender's offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub name: String,
    pub size: u64,
}

/// A finished receive.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CodeReceipt {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Bytes that were already here from an earlier, interrupted try.
    pub resumed_from: u64,
    pub seconds: f64,
    pub megabytes_per_second: f64,
    /// The sender's endpoint id: who proved the code. Never trust by name.
    pub sender_id: String,
    /// What the sender calls itself (empty if it didn't say): a label, not
    /// proof of anything.
    pub sender_name: String,
}

pub(crate) fn connection(reason: impl std::fmt::Display) -> RelayError {
    RelayError::Connection {
        reason: reason.to_string(),
    }
}

pub(crate) async fn put(send: &mut SendStream, bytes: &[u8]) -> Result<(), RelayError> {
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(connection)?;
    send.write_all(bytes).await.map_err(connection)
}

pub(crate) async fn get_within(
    recv: &mut RecvStream,
    limit: Duration,
) -> Result<Vec<u8>, RelayError> {
    let read = async {
        let mut len = [0u8; 4];
        recv.read_exact(&mut len).await.map_err(connection)?;
        let len = u32::from_be_bytes(len) as usize;
        if len > 64 * 1024 {
            return Err(connection("message too long"));
        }
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await.map_err(connection)?;
        Ok(buf)
    };
    tokio::time::timeout(limit, read)
        .await
        .map_err(|_| connection("the sender stopped answering"))?
}

/// Why a receive ended without the file, and what the sender had offered
/// by then, if anything (for the history).
#[derive(Debug)]
pub struct Failed {
    pub error: RelayError,
    pub offered: Option<Offered>,
}

impl From<RelayError> for Failed {
    fn from(error: RelayError) -> Self {
        Self {
            error,
            offered: None,
        }
    }
}

/// Receives the file or folder a sender offers with `code`. `addresses` are
/// the sender's "host:port"s from Bonjour and `endpoint_id` its id (both
/// from its advertisement). It lands in `inbox`; blobs stay in the node's
/// store, so a later try resumes.
#[allow(clippy::too_many_arguments)]
pub async fn receive(
    code: &str,
    addresses: &[String],
    endpoint_id: &str,
    node: Arc<Node>,
    inbox: &Path,
    my_name: String,
    progress: Arc<CodeProgress>,
    stop: Arc<AtomicBool>,
) -> Result<CodeReceipt, Failed> {
    let code = WordCode::parse(code)?;
    let sender: EndpointId = endpoint_id.parse().map_err(|_| RelayError::Unreachable {
        reason: "the sender's id is malformed".into(),
    })?;
    let addrs: Vec<TransportAddr> = addresses
        .iter()
        .filter_map(|a| a.parse::<SocketAddr>().ok())
        .map(TransportAddr::Ip)
        .collect();
    if addrs.is_empty() {
        return Err(RelayError::Unreachable {
            reason: "no address for the sender".into(),
        }
        .into());
    }
    let target = EndpointAddr::from_parts(sender, addrs);
    let inbox = inbox.to_path_buf();
    progress.active.store(true, Ordering::SeqCst);
    progress.done.store(0, Ordering::SeqCst);
    let offered: Arc<std::sync::Mutex<Option<Offered>>> = Arc::default();
    // iroh needs tokio; the caller (UniFFI's executor) may not be tokio, so
    // the work runs on Relay's own runtime and this only awaits it.
    let work = TOKIO.spawn({
        let progress = Arc::clone(&progress);
        let offered = Arc::clone(&offered);
        async move {
            tokio::select! {
                r = run(code, target, &node, &inbox, &my_name, &progress, &offered) => r,
                _ = async {
                    while !stop.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                } => Err(RelayError::Connection { reason: "cancelled".into() }),
            }
        }
    });
    let result = work.await.unwrap_or_else(|e| Err(connection(e)));
    progress.active.store(false, Ordering::SeqCst);
    result.map_err(|error| Failed {
        error,
        offered: crate::node::lock(&offered).clone(),
    })
}

async fn run(
    code: WordCode,
    target: EndpointAddr,
    node: &Node,
    inbox: &Path,
    my_name: &str,
    progress: &CodeProgress,
    offered: &std::sync::Mutex<Option<Offered>>,
) -> Result<CodeReceipt, RelayError> {
    let sender = target.id;
    let endpoint = &node.endpoint;
    let me = endpoint.id();
    async {
        let conn = tokio::time::timeout(STEP, endpoint.connect(target, CODE_ALPN))
            .await
            .map_err(|_| RelayError::Unreachable {
                reason: "the sender didn't answer".into(),
            })?
            .map_err(|e| RelayError::Unreachable {
                reason: e.to_string(),
            })?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(connection)?;
        let (state, my_msg) = code.pake();
        put(&mut send, &my_msg).await?;
        let first = get_within(&mut recv, STEP).await?;
        let refusal = match first.first() {
            Some(&GO) => None,
            Some(&BUSY) => Some("the sender is busy with someone else; try again in a moment"),
            Some(&GONE) => Some("that code has been used or has expired; ask for a new one"),
            _ => Some("the sender answered something unexpected"),
        };
        if let Some(reason) = refusal {
            conn.close(0u32.into(), b"ok");
            return Err(RelayError::Declined {
                reason: reason.into(),
            });
        }
        let key = state
            .finish(&first[1..])
            .map_err(|_| RelayError::Declined {
                reason: "wrong code; ask the sender for the new one".into(),
            })?;
        put(&mut send, &confirm(&key, "receiver", sender, me)).await?;
        let theirs = get_within(&mut recv, STEP)
            .await
            .map_err(|_| RelayError::Declined {
                reason: "wrong code; ask the sender for the new one".into(),
            })?;
        if theirs != confirm(&key, "sender", sender, me) {
            return Err(RelayError::Declined {
                reason: "the sender couldn't prove the code".into(),
            });
        }
        let offer = get_within(&mut recv, STEP).await?;
        let (hash, size, collection, name) = parse_offer(&offer)?;
        *crate::node::lock(offered) = Some(Offered {
            name: name.clone(),
            size,
        });
        let content = if collection {
            HashAndFormat::hash_seq(hash)
        } else {
            HashAndFormat::raw(hash)
        };
        let store = &node.store;
        let already = store
            .remote()
            .local(content)
            .await
            .map(|l| l.local_bytes())
            .unwrap_or(0);
        progress.total.store(size, Ordering::SeqCst);
        progress.already.store(already, Ordering::SeqCst);
        progress.done.store(already, Ordering::SeqCst);
        log::info!("code receive: {size} bytes, {already} already here");
        let started = Instant::now();
        // Tell the sender we're still here while the file moves.
        let (stop_ticks, mut stopped) = tokio::sync::oneshot::channel::<()>();
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
        // Fetch, unless the sender closes the code connection first (it
        // was stopped): then stop too, keeping what arrived for a resume.
        hold(node, content).await?;
        let downloaded = tokio::select! {
            r = fetch(node, content, sender, progress) => r,
            _ = conn.closed() => Err(RelayError::Declined {
                reason: "the sender stopped".into(),
            }),
        };
        let seconds = started.elapsed().as_secs_f64();
        let saved = match downloaded {
            Ok(()) => place(node, hash, collection, inbox, &name).await,
            Err(e) => Err(e),
        };
        let _ = stop_ticks.send(());
        let mut send = ticker.await.map_err(connection)?;
        let out = saved?;
        release(node, hash).await;
        put(&mut send, b"done").await?;
        // Names last, once the transfer is safe: each side learns what the
        // other calls itself, for "Trust this device?". An older sender
        // just closes.
        let _ = put(&mut send, &named(my_name)).await;
        let sender_name = their_name(&mut recv).await;
        let _ = send.finish();
        let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
        let fetched = size.saturating_sub(already);
        Ok(CodeReceipt {
            name,
            path: out.to_string_lossy().into_owned(),
            size,
            resumed_from: already,
            seconds,
            megabytes_per_second: if seconds > 0.0 {
                fetched as f64 / seconds / 1e6
            } else {
                0.0
            },
            sender_id: sender.to_string(),
            sender_name,
        })
    }
    .await
}

/// The message that tells the other side of a code transfer this device's
/// name.
pub(crate) fn named(name: &str) -> Vec<u8> {
    [b"me:".as_slice(), name.as_bytes()].concat()
}

/// The other side's name, if it says it within a moment; empty otherwise.
pub(crate) async fn their_name(recv: &mut RecvStream) -> String {
    match get_within(recv, Duration::from_secs(2)).await {
        Ok(m) if m.starts_with(b"me:") => {
            crate::device::clean_offered_name(&String::from_utf8_lossy(&m[3..]), "")
        }
        _ => String::new(),
    }
}

/// Reads an offer: the root hash, the size, whether it is a folder, and
/// its name (a name, never a path).
fn parse_offer(offer: &[u8]) -> Result<(Hash, u64, bool, String), RelayError> {
    if offer.len() < 40 {
        return Err(connection("bad offer"));
    }
    let hash = Hash::from_bytes(offer[..32].try_into().expect("32 bytes"));
    let size = u64::from_be_bytes(offer[32..40].try_into().expect("8 bytes"));
    let (collection, name) = match offer.get(40) {
        Some(0) => match offer.get(41) {
            Some(&COLLECTION) => (true, &offer[42..]),
            _ => {
                return Err(RelayError::Declined {
                    reason: "the sender's Relay is newer; update this one".into(),
                })
            }
        },
        _ => (false, &offer[40..]),
    };
    let name = crate::device::clean_offered_name(
        &String::from_utf8_lossy(name),
        if collection { "folder" } else { "file" },
    );
    Ok((hash, size, collection, name))
}

/// Fetches `content` from the sender, verifying every chunk, and keeps
/// `progress.done` current.
pub(crate) async fn fetch(
    node: &Node,
    content: HashAndFormat,
    sender: EndpointId,
    progress: &CodeProgress,
) -> Result<(), RelayError> {
    let failed = |e: &dyn std::fmt::Display| connection(format!("{e:#}"));
    let mut items = node
        .store
        .downloader(&node.endpoint)
        .download(content, Some(sender))
        .stream()
        .await
        .map_err(|e| failed(&e))?;
    while let Some(item) = items.next().await {
        match item {
            DownloadProgressItem::Progress(here) => {
                progress.done.store(here, Ordering::SeqCst);
                #[cfg(test)]
                {
                    let at = progress.stop_at.load(Ordering::SeqCst);
                    if at > 0 && here >= at {
                        return Err(connection("stopped on purpose (test)"));
                    }
                }
            }
            DownloadProgressItem::Error(e) => return Err(failed(&e)),
            DownloadProgressItem::DownloadError => return Err(connection("the download failed")),
            _ => {}
        }
    }
    Ok(())
}

/// Tags a receive's content before fetching it, so the store keeps what
/// arrives (for a resume, even after a relaunch) until `release`. One named
/// tag per receive, on its root: a folder's files are kept through it.
pub(crate) async fn hold(node: &Node, content: HashAndFormat) -> Result<(), RelayError> {
    let name = format!("{RECEIVE_TAG}{}/{}", content.hash, forge_runtime::now_ms());
    node.store
        .tags()
        .set(name, content)
        .await
        .map_err(file_error)
}

/// Lets go of a receive's content once it is saved: the store's copy goes
/// at its next clean-up.
pub(crate) async fn release(node: &Node, hash: Hash) {
    let prefix = format!("{RECEIVE_TAG}{hash}/");
    if let Err(err) = node.store.tags().delete_prefix(prefix).await {
        log::warn!("relay couldn't let go of a received item: {err}");
    }
}

const RECEIVE_TAG: &str = "relay/receive/";

/// Drops receive tags older than `RECEIVE_KEPT_MS` (a transfer that never
/// came back), when the store opens.
pub(crate) async fn let_go_of_old_receives(store: &iroh_blobs::store::fs::FsStore) {
    let oldest = forge_runtime::now_ms() - crate::node::RECEIVE_KEPT_MS;
    let Ok(mut tags) = store.tags().list_prefix(RECEIVE_TAG).await else {
        return;
    };
    let mut old = Vec::new();
    while let Some(Ok(tag)) = tags.next().await {
        let name = String::from_utf8_lossy(tag.name.as_ref()).into_owned();
        let at = name.rsplit('/').next().and_then(|t| t.parse::<i64>().ok());
        if at.is_none_or(|at| at < oldest) {
            old.push(name);
        }
    }
    for name in old {
        let _ = store.tags().delete(name).await;
    }
}

/// Saves what arrived as `name` in `inbox` and returns where. A file or
/// folder already there with the same name and the same content (a retry,
/// or the same thing sent again) is reused, not copied a second time;
/// different content with the same name gets the next free name, `name (2)`
/// and so on.
pub(crate) async fn place(
    node: &Node,
    hash: Hash,
    collection: bool,
    inbox: &Path,
    name: &str,
) -> Result<PathBuf, RelayError> {
    std::fs::create_dir_all(inbox).map_err(file_error)?;
    let files = if collection {
        let list = Collection::load(hash, &*node.store)
            .await
            .map_err(file_error)?;
        list.iter()
            .map(|(inside, file)| (inside.clone(), *file))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for candidate in names(inbox, name) {
        if !candidate.exists() {
            save(node, hash, collection, inbox, &candidate).await?;
            return Ok(candidate);
        }
        let same = {
            let (candidate, files) = (candidate.clone(), files.clone());
            tokio::task::spawn_blocking(move || {
                if collection {
                    candidate.is_dir()
                        && files.iter().all(|(inside, file)| {
                            inside_folder(&candidate, inside)
                                .is_some_and(|path| same_content(&path, *file))
                        })
                } else {
                    same_content(&candidate, hash)
                }
            })
            .await
            .unwrap_or(false)
        };
        if same {
            log::info!("relay: received again, already saved");
            return Ok(candidate);
        }
    }
    unreachable!("names() never ends")
}

/// `inbox/name`, then `name (2)`, `name (3)` and so on.
fn names<'a>(inbox: &'a Path, name: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    std::iter::once(inbox.join(name))
        .chain((2..).map(move |i| inbox.join(format!("{stem} ({i}){ext}"))))
}

/// Whether the file at `path` is exactly the blob `hash` (BLAKE3, read in
/// pieces).
fn same_content(path: &Path, hash: Hash) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if !path.is_file() {
        return false;
    }
    let mut hasher = blake3::Hasher::new();
    if hasher.update_reader(file).is_err() {
        return false;
    }
    hasher.finalize().as_bytes() == hash.as_bytes()
}

/// Copies what arrived out of the store: a file to `out`, a folder's files
/// under the folder `out`, each kept inside it whatever its path says.
pub(crate) async fn save(
    node: &Node,
    hash: Hash,
    collection: bool,
    inbox: &Path,
    out: &Path,
) -> Result<(), RelayError> {
    std::fs::create_dir_all(inbox).map_err(file_error)?;
    let blobs = node.store.blobs();
    if !collection {
        blobs.export(hash, out).await.map_err(file_error)?;
        return Ok(());
    }
    let list = Collection::load(hash, &*node.store)
        .await
        .map_err(file_error)?;
    std::fs::create_dir_all(out).map_err(file_error)?;
    for (inside, file) in list.iter() {
        let target = inside_folder(out, inside).ok_or_else(|| RelayError::File {
            reason: format!("the sender named a file outside the folder: {inside}"),
        })?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(file_error)?;
        }
        blobs.export(*file, &target).await.map_err(file_error)?;
    }
    Ok(())
}

/// `folder` joined with a `/`-separated path from a sender, or `None` if the
/// path is empty or could leave the folder (`..`, a root, a drive).
fn inside_folder(folder: &Path, inside: &str) -> Option<PathBuf> {
    let mut path = folder.to_path_buf();
    let mut any = false;
    for part in inside.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            _ if part.contains('\\') || part.contains(':') || part.contains('\0') => return None,
            _ => {
                path.push(part);
                any = true;
            }
        }
    }
    any.then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_a_number_and_three_list_words() {
        let code = WordCode::parse(" 37-Grasp gag-drum ").unwrap();
        assert_eq!(code.number, 37);
        assert!(WordCode::parse("37-grasp-gag").is_err());
        assert!(WordCode::parse("0-grasp-gag-drum").is_err());
        assert!(WordCode::parse("37-grasp-gag-zzzzz").is_err());
    }

    #[test]
    fn both_sides_derive_the_same_key_from_the_same_words() {
        let a = WordCode::parse("37-grasp-gag-drum").unwrap();
        let b = WordCode::parse("37-grasp-gag-drum").unwrap();
        let (sa, ma) = a.pake();
        let (sb, mb) = b.pake();
        assert_eq!(sa.finish(&mb).unwrap(), sb.finish(&ma).unwrap());
    }

    #[test]
    fn a_new_code_parses_and_avoids_numbers_in_use() {
        let in_use: Vec<u8> = (1..=98).collect();
        for _ in 0..20 {
            let code = WordCode::new(&in_use);
            assert_eq!(code.number, 99);
            assert_eq!(WordCode::parse(&code.text()).unwrap(), code);
        }
        let all: Vec<u8> = (1..=99).collect();
        assert!((1..=99).contains(&WordCode::new(&all).number));
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let a: Vec<_> = names(dir.path(), "a.txt").take(3).collect();
        assert_eq!(
            a,
            [
                dir.path().join("a.txt"),
                dir.path().join("a (2).txt"),
                dir.path().join("a (3).txt")
            ]
        );
        assert_eq!(
            names(dir.path(), "b").nth(1),
            Some(dir.path().join("b (2)"))
        );
        // A file is its BLAKE3 hash, read in pieces.
        let file = dir.path().join("a.txt");
        assert!(same_content(&file, Hash::new(b"x")));
        assert!(!same_content(&file, Hash::new(b"y")));
        assert!(!same_content(&dir.path().join("gone"), Hash::new(b"x")));
    }

    #[test]
    fn a_folder_never_writes_outside_itself() {
        let f = Path::new("/in/box");
        assert_eq!(inside_folder(f, "a/b.txt"), Some(f.join("a").join("b.txt")));
        assert_eq!(inside_folder(f, "./a"), Some(f.join("a")));
        assert_eq!(inside_folder(f, "../etc/passwd"), None);
        assert_eq!(inside_folder(f, "a/../../x"), None);
        assert_eq!(inside_folder(f, "c:\\x"), None);
        assert_eq!(inside_folder(f, ""), None);
        assert_eq!(inside_folder(f, "/"), None);
    }

    #[test]
    fn offers_name_a_file_or_a_folder() {
        let mut offer = vec![7u8; 32];
        offer.extend_from_slice(&5u64.to_be_bytes());
        let mut file = offer.clone();
        file.extend_from_slice(b"../a.txt");
        let (_, size, collection, name) = parse_offer(&file).unwrap();
        assert_eq!((size, collection, name.as_str()), (5, false, "a.txt"));
        let mut folder = offer.clone();
        folder.extend_from_slice(&[0, COLLECTION]);
        folder.extend_from_slice(b"Photos");
        let (_, _, collection, name) = parse_offer(&folder).unwrap();
        assert!(collection);
        assert_eq!(name, "Photos");
        let mut newer = offer;
        newer.extend_from_slice(&[0, 9]);
        assert!(parse_offer(&newer).is_err());
    }
}
