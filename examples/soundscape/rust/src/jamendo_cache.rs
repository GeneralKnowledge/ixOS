//! Jamendo's answers kept on the phone, so a page opened again, or a
//! search asked again, doesn't ask Jamendo again.
//!
//! [`JamendoAnswers`] sits in front of the app's HTTP client. Every other
//! request passes straight through; a `GET` to Jamendo's API is answered
//! from what was kept while that is fresh ([`FRESH_MS`]). An older answer is
//! asked again, and is still used when Jamendo can't be reached, answers
//! with a server error, or answers with nothing, so a page that opened
//! once still opens offline. Only answers with something in them are kept,
//! so Jamendo's occasional empty answer (`jamendo::results_at`) is never
//! remembered. The key is the request's address without the client ID, so
//! the ID is never written to disk.
//!
//! Answers live in memory and, one file each, in the caches folder, which
//! iOS may empty; nothing here is the person's own data. Files older than
//! [`KEEP_MS`] are removed when the app opens, and the newest
//! [`MAX_FILES`] are kept. [`JamendoAnswers::refresh`] (pull to refresh)
//! makes everything kept so far stale, without throwing it away.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use forge_net::{CancelHandle, HttpClient, Method, NetError, Request, Response, StreamingResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::jamendo::API;

/// How long a kept answer is used without asking Jamendo: six hours.
pub const FRESH_MS: u64 = 6 * 60 * 60 * 1000;
/// How long a kept answer is kept at all, for when Jamendo can't be
/// reached: 30 days.
pub const KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// The most answers kept on disk; the oldest go first.
pub const MAX_FILES: usize = 500;
/// The largest answer kept.
const MAX_KEPT_BYTES: usize = 2 * 1024 * 1024;
/// The most answers held in memory; older ones are read from disk again.
pub const MAX_IN_MEMORY: usize = 100;

/// Milliseconds since 1970, swappable for tests.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

#[derive(Clone, Serialize, Deserialize)]
struct Kept {
    saved_ms: u64,
    key: String,
    body: String,
    /// The refresh it was kept under; one read from disk counts as kept
    /// before any refresh in this run.
    #[serde(skip, default)]
    generation: u64,
}

/// The app's HTTP client with Jamendo's answers kept. See the module
/// documentation.
pub struct JamendoAnswers {
    inner: Arc<dyn HttpClient>,
    dir: Option<PathBuf>,
    memory: Mutex<HashMap<String, Kept>>,
    /// Answers kept under an older refresh are stale, whatever their age.
    generation: AtomicU64,
    now: Clock,
}

impl JamendoAnswers {
    /// Keeps answers in memory and in `dir` (created when needed; `None`
    /// keeps them in memory only), dropping files there that are too old.
    pub fn new(inner: Arc<dyn HttpClient>, dir: Option<PathBuf>, now: Clock) -> Self {
        if let Some(dir) = &dir {
            sweep(dir, now());
        }
        Self {
            inner,
            dir,
            memory: Mutex::default(),
            generation: AtomicU64::new(0),
            now,
        }
    }

    /// Makes every answer kept so far stale: the next ask of each goes to
    /// Jamendo, and the old answer is used only if that fails.
    pub fn refresh(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn fresh(&self, kept: &Kept, now: u64) -> bool {
        kept.generation == self.generation.load(Ordering::SeqCst)
            && now.saturating_sub(kept.saved_ms) < FRESH_MS
    }

    fn lookup(&self, key: &str, now: u64) -> Option<Kept> {
        let mut memory = self.memory.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(kept) = memory.get(key) {
            return Some(kept.clone());
        }
        let path = self.dir.as_ref()?.join(file_name(key));
        let kept: Kept = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if kept.key != key || now.saturating_sub(kept.saved_ms) >= KEEP_MS {
            return None;
        }
        remember(&mut memory, key.to_string(), kept.clone());
        Some(kept)
    }

    fn keep(&self, key: String, now: u64, generation: u64, body: &[u8]) {
        let Ok(body) = String::from_utf8(body.to_vec()) else {
            return;
        };
        let kept = Kept {
            saved_ms: now,
            key: key.clone(),
            body,
            generation,
        };
        if let Some(dir) = &self.dir {
            let written = serde_json::to_vec(&kept)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    forge_storage::write_atomic(&dir.join(file_name(&key)), &bytes)
                        .map_err(|e| e.to_string())
                });
            if let Err(err) = written {
                forge_runtime::logging::warn!("couldn't keep a Jamendo answer: {err}");
            }
        }
        let mut memory = self.memory.lock().unwrap_or_else(|p| p.into_inner());
        remember(&mut memory, key, kept);
    }
}

/// Holds `kept` in memory, letting the oldest go past [`MAX_IN_MEMORY`]
/// (its file stays on disk).
fn remember(memory: &mut HashMap<String, Kept>, key: String, kept: Kept) {
    memory.insert(key, kept);
    while memory.len() > MAX_IN_MEMORY {
        let Some(oldest) = memory
            .iter()
            .min_by_key(|(_, k)| k.saved_ms)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        memory.remove(&oldest);
    }
}

impl HttpClient for JamendoAnswers {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        self.send_cancellable(request, &CancelHandle::new())
    }

    fn send_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        let Some(key) = key(request) else {
            return self.inner.send_cancellable(request, cancel);
        };
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let now = (self.now)();
        // Read before asking, so a refresh while Jamendo answers leaves
        // this answer stale.
        let generation = self.generation.load(Ordering::SeqCst);
        let kept = self.lookup(&key, now);
        if let Some(kept) = &kept {
            if self.fresh(kept, now) {
                return Ok(Response::new(200, kept.body.clone()));
            }
        }
        let fallback = |kept: Option<Kept>| kept.map(|k| Response::new(200, k.body));
        match self.inner.send_cancellable(request, cancel) {
            Ok(response) if (200..300).contains(&response.status) => {
                if worth_keeping(&response.body) {
                    self.keep(key, now, generation, &response.body);
                    Ok(response)
                } else if answered_nothing(&response.body) {
                    Ok(fallback(kept).unwrap_or(response))
                } else {
                    // Jamendo refused (a wrong client ID): say so.
                    Ok(response)
                }
            }
            Ok(response) if response.status >= 500 => Ok(fallback(kept).unwrap_or(response)),
            Ok(response) => Ok(response),
            Err(NetError::Cancelled) => Err(NetError::Cancelled),
            Err(err) => fallback(kept).ok_or(err),
        }
    }

    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        self.inner.open_stream(request)
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        self.inner.open_stream_cancellable(request, cancel)
    }

    fn send_file(
        &self,
        request: &Request,
        path: &Path,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        self.inner.send_file(request, path, cancel)
    }
}

/// The key for a `GET` to Jamendo's API: its address without the client
/// ID. `None` for anything else.
pub fn key(request: &Request) -> Option<String> {
    if request.method != Method::Get || !request.url.starts_with(&format!("{API}/")) {
        return None;
    }
    let (path, query) = request
        .url
        .split_once('?')
        .unwrap_or((request.url.as_str(), ""));
    let rest: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty() && !pair.starts_with("client_id="))
        .collect();
    Some(if rest.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", rest.join("&"))
    })
}

fn file_name(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}.json")
}

fn parsed(body: &[u8]) -> Option<Value> {
    if body.len() > MAX_KEPT_BYTES {
        return None;
    }
    serde_json::from_slice(body).ok()
}

fn succeeded(json: &Value) -> bool {
    json["headers"]["status"]
        .as_str()
        .is_none_or(|s| s == "success")
}

/// A successful answer with something in it.
fn worth_keeping(body: &[u8]) -> bool {
    parsed(body).is_some_and(|json| {
        succeeded(&json) && json["results"].as_array().is_some_and(|r| !r.is_empty())
    })
}

/// A successful answer with nothing in it.
fn answered_nothing(body: &[u8]) -> bool {
    parsed(body).is_some_and(|json| {
        succeeded(&json) && json["results"].as_array().is_none_or(|r| r.is_empty())
    })
}

/// Removes kept answers older than [`KEEP_MS`], then the oldest beyond
/// [`MAX_FILES`].
fn sweep(dir: &Path, now: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(now);
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or_default();
        if age >= Duration::from_millis(KEEP_MS) {
            let _ = std::fs::remove_file(&path);
        } else {
            files.push((modified, path));
        }
    }
    if files.len() > MAX_FILES {
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        for (_, path) in files.drain(MAX_FILES..) {
            let _ = std::fs::remove_file(path);
        }
    }
}
