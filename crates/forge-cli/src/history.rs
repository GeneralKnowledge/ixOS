//! The app's command history: one JSON line per `forge` command run on an
//! app, by a person or a script, in `<app>/.forge/history.jsonl`
//! (`forge.history/1`). `forge verify --run` reads it next to the `forge
//! mcp` journal, so a run record shows the builds and installs a person ran
//! at the terminal, not only an agent's tool calls.
//!
//! A line holds the command, when, whether it passed, its stage and error
//! (cut to [`MAX_ERROR`]) and its log file. Never arguments or app output.
//! Commands run by `forge mcp` are left out: its journal records them.
//! `.forge/` is git-ignored. Writing is best effort: a history that can't be
//! written never fails the command.

use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};

pub const SCHEMA: &str = "forge.history/1";
pub const FILE: &str = ".forge/history.jsonl";

/// Error text kept per line; the rest is in the command's log.
const MAX_ERROR: usize = 500;

/// One finished command.
pub struct Entry<'a> {
    pub command: &'a str,
    pub ok: bool,
    pub stage: &'a str,
    pub error: Option<&'a str>,
    pub duration_ms: u64,
    pub log: Option<&'a Path>,
}

impl Entry<'_> {
    pub fn to_json(&self, at: &str) -> Value {
        json!({
            "schema": SCHEMA,
            "event": "call",
            "via": "cli",
            "at": at,
            "tool": self.command,
            "ok": self.ok,
            "stage": self.stage,
            "error": self.error.map(|e| e.chars().take(MAX_ERROR).collect::<String>()),
            "duration_ms": self.duration_ms,
            "log": self.log.map(|p| p.display().to_string()),
        })
    }
}

/// Appends `entry` to the app's history, unless this command runs under
/// `forge mcp`. `app` must hold a forge.toml.
pub fn append(app: &Path, entry: &Entry) {
    if std::env::var_os("FORGE_MCP_CHILD").is_some() || !app.join("forge.toml").is_file() {
        return;
    }
    append_to(&app.join(FILE), entry, &crate::report::timestamp());
}

fn append_to(path: &Path, entry: &Entry, at: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}", entry.to_json(at));
    }
}

/// The history lines at or after `since` (a run id, UTC).
pub fn since(app: &Path, since: &str) -> Vec<Value> {
    std::fs::read_to_string(app.join(FILE))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["at"].as_str().is_some_and(|at| at >= since))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_and_read_back_from_a_time() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(FILE);
        let long = "x".repeat(2000);
        let failed = Entry {
            command: "build ios",
            ok: false,
            stage: "swift",
            error: Some(&long),
            duration_ms: 12,
            log: None,
        };
        append_to(&path, &failed, "2026-11-09T10-00-00");
        let passed = Entry {
            command: "build ios",
            ok: true,
            stage: "ok",
            error: None,
            duration_ms: 9,
            log: None,
        };
        append_to(&path, &passed, "2026-11-09T11-00-00");
        let all = since(tmp.path(), "2026-11-09T00-00-00");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0]["tool"], "build ios");
        assert_eq!(all[0]["via"], "cli");
        assert_eq!(all[0]["error"].as_str().unwrap().len(), MAX_ERROR);
        let later = since(tmp.path(), "2026-11-09T10-30-00");
        assert_eq!(later.len(), 1);
        assert_eq!(later[0]["ok"], true);
    }
}
