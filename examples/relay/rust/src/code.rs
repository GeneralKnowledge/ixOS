//! Receiving a file with a word code, the transfer Relay is moving to
//! (docs/relay-platform-plan.md, M1). The sender is the M0 spike
//! (`examples/relay/spike`, `relay-spike send <file>`) on a computer; this is
//! the same receiver, so the iPhone can prove the stack works there:
//!
//! - iroh endpoints with relays off: the local network only.
//! - The app finds the sender with Bonjour (`_forgerelay._udp`, its TXT
//!   record carries the code's number and the sender's endpoint id) and hands
//!   the addresses here; raw multicast needs an entitlement a free team
//!   can't have, so Rust does not browse on iOS.
//! - SPAKE2 with the code's three words, then a key confirmation over both
//!   endpoint ids. A wrong code is spent on the sender's side.
//! - Then iroh-blobs fetches the file, verifying every chunk, into a store
//!   on disk, so receiving again with the sender's next code resumes.
//!
//! The wire protocol matches the spike's exactly. The sending side, which
//! the iPhone uses too, is `code_send`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use iroh::endpoint::{presets, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, TransportAddr};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{Hash, HashAndFormat};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::model::RelayError;

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

/// Receives the file a sender offers with `code`. `addresses` are the
/// sender's "host:port"s from Bonjour and `endpoint_id` its id (both from
/// its advertisement). The file lands in `inbox`; blobs are kept in `store`
/// so a later try resumes.
pub async fn receive(
    code: &str,
    addresses: &[String],
    endpoint_id: &str,
    store: &Path,
    inbox: &Path,
    progress: Arc<CodeProgress>,
    stop: Arc<AtomicBool>,
) -> Result<CodeReceipt, RelayError> {
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
        });
    }
    let target = EndpointAddr::from_parts(sender, addrs);
    let store = store.to_path_buf();
    let inbox = inbox.to_path_buf();
    progress.active.store(true, Ordering::SeqCst);
    // iroh needs tokio; the caller (UniFFI's executor) may not be tokio, so
    // the work runs on Relay's own runtime and this only awaits it.
    let work = TOKIO.spawn({
        let progress = Arc::clone(&progress);
        async move {
            tokio::select! {
                r = run(code, target, &store, &inbox, &progress) => r,
                _ = async {
                    while !stop.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                } => Err(RelayError::Connection { reason: "cancelled".into() }),
            }
        }
    });
    let result = work.await.unwrap_or_else(|e| Err(connection(e)));
    progress.active.store(false, Ordering::SeqCst);
    result
}

async fn run(
    code: WordCode,
    target: EndpointAddr,
    store_dir: &Path,
    inbox: &Path,
    progress: &CodeProgress,
) -> Result<CodeReceipt, RelayError> {
    let sender = target.id;
    let endpoint = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .map_err(|e| RelayError::Unreachable {
            reason: e.to_string(),
        })?;
    let me = endpoint.id();
    let outcome = async {
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
        if offer.len() < 40 {
            return Err(connection("bad offer"));
        }
        let hash = Hash::from_bytes(offer[..32].try_into().expect("32 bytes"));
        let size = u64::from_be_bytes(offer[32..40].try_into().expect("8 bytes"));
        // A name, never a path.
        let name = Path::new(&String::from_utf8_lossy(&offer[40..]).to_string())
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("file")
            .to_string();

        let store = FsStore::load(store_dir)
            .await
            .map_err(|e| RelayError::File {
                reason: e.to_string(),
            })?;
        let already = store
            .remote()
            .local(HashAndFormat::raw(hash))
            .await
            .map(|l| l.local_bytes())
            .unwrap_or(0);
        progress.total.store(size, Ordering::SeqCst);
        progress.already.store(already, Ordering::SeqCst);
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
        let downloaded = store
            .downloader(&endpoint)
            .download(hash, Some(sender))
            .await;
        let seconds = started.elapsed().as_secs_f64();
        let out = unique_path(inbox, &name);
        let saved = match downloaded {
            Ok(()) => {
                std::fs::create_dir_all(inbox).map_err(|e| RelayError::File {
                    reason: e.to_string(),
                })?;
                store
                    .blobs()
                    .export(hash, &out)
                    .await
                    .map(|_| ())
                    .map_err(|e| RelayError::File {
                        reason: e.to_string(),
                    })
            }
            Err(e) => Err(connection(format!("{e:#}"))),
        };
        let _ = stop_ticks.send(());
        let mut send = ticker.await.map_err(connection)?;
        saved?;
        put(&mut send, b"done").await?;
        let _ = send.finish();
        let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
        store.shutdown().await.ok();
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
        })
    }
    .await;
    endpoint.close().await;
    outcome
}

/// `inbox/name`, or `inbox/name (2)` and so on if that is taken.
fn unique_path(inbox: &Path, name: &str) -> PathBuf {
    let first = inbox.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..)
        .map(|i| inbox.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("a free name")
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
        assert_eq!(
            unique_path(dir.path(), "a.txt"),
            dir.path().join("a (2).txt")
        );
        assert_eq!(unique_path(dir.path(), "b"), dir.path().join("b"));
    }
}
