//! A start marker and heartbeat for long commands (`forge run ios`), so
//! whoever asked for the run (a person, an agent, another session over
//! Remote Control) can tell within seconds that it really started and what
//! it is doing, instead of assuming it is building (Good Dog pain point 18,
//! 2026-11-08: a laptop request was refused, nothing ran, and Reuben waited
//! 20 minutes at the phone).
//!
//! `<app>/.forge/status/<command>.json`:
//!
//! ```json
//! {"schema": "forge.status/1", "command": "run ios", "pid": 4242,
//!  "phase": "building", "started": 1794139200, "updated": 1794139260,
//!  "log": ".forge/logs/…-run-ios.log"}
//! ```
//!
//! `updated` moves every [`BEAT`] while the command lives. A file older than
//! the request, or an `updated` more than a minute old with a phase that
//! isn't `done` or `failed`, means nothing is running.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

pub const BEAT: Duration = Duration::from_secs(10);

pub struct Status {
    path: PathBuf,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

struct State {
    command: String,
    phase: String,
    started: i64,
    log: Option<String>,
    error: Option<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn write(path: &Path, s: &State) {
    let doc = json!({
        "schema": "forge.status/1",
        "command": s.command,
        "pid": std::process::id(),
        "phase": s.phase,
        "started": s.started,
        "updated": now(),
        "log": s.log,
        "error": s.error,
    });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written whole and renamed, so a reader never sees half a file.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, doc.to_string() + "\n").is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

impl Status {
    /// Writes the marker (phase `starting`) and starts the heartbeat.
    pub fn start(app_dir: &Path, command: &str) -> Status {
        let slug: String = command
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let path = app_dir.join(".forge/status").join(format!("{slug}.json"));
        let state = Arc::new(Mutex::new(State {
            command: command.to_owned(),
            phase: "starting".into(),
            started: now(),
            log: crate::report::log_path().map(|p| p.display().to_string()),
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        write(&path, &state.lock().unwrap_or_else(|p| p.into_inner()));
        {
            let (path, state, stop) = (path.clone(), state.clone(), stop.clone());
            let _ = std::thread::Builder::new()
                .name("forge-status".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(500));
                        let s = state.lock().unwrap_or_else(|p| p.into_inner());
                        let elapsed = now() - s.started;
                        if elapsed % BEAT.as_secs() as i64 == 0 {
                            write(&path, &s);
                        }
                    }
                });
        }
        Status { path, state, stop }
    }

    /// The command moved on: `building`, `waiting_for_device`, `installing`.
    pub fn phase(&self, phase: &str) {
        println!("[forge] status: {phase} ({})", self.path.display());
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        s.phase = phase.to_owned();
        write(&self.path, &s);
    }

    /// Ends the heartbeat with `done`, or `failed` and the error.
    pub fn finish(self, result: &Result<(), String>) {
        self.stop.store(true, Ordering::Relaxed);
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(()) => s.phase = "done".into(),
            Err(e) => {
                s.phase = "failed".into();
                s.error = Some(e.chars().take(500).collect());
            }
        }
        write(&self.path, &s);
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_the_start_each_phase_and_the_end() {
        let tmp = tempfile::tempdir().unwrap();
        let status = Status::start(tmp.path(), "run ios");
        let path = status.path().to_path_buf();
        assert!(path.ends_with(".forge/status/run-ios.json"));
        let read = || -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        };
        assert_eq!(read()["phase"], "starting");
        assert_eq!(read()["pid"], std::process::id());
        status.phase("building");
        assert_eq!(read()["phase"], "building");
        status.finish(&Err("xtool dev build failed".into()));
        let end = read();
        assert_eq!(end["phase"], "failed");
        assert_eq!(end["error"], "xtool dev build failed");
    }
}
