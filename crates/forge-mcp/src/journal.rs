//! The run journal: one JSON line per tool call, so a person (or the next
//! agent) can see afterwards what an agent did, in what order, how long each
//! step took and where it failed, without the client's transcript.
//!
//! `<root>/.forge/runs/<UTC start>-mcp/journal.jsonl`, created on the first
//! tool call (a server that only lists tools writes nothing). `.forge/` is
//! git-ignored. Format `forge.journal/1` (docs/forge-mcp.md#the-run-journal).
//!
//! What it never holds: text the agent typed into the app, or values it
//! expected to read back (stored as their length only), app output (trees,
//! logs, screenshots stay in the files the result already points at), and
//! anything about the Apple account.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

pub const SCHEMA: &str = "forge.journal/1";

/// Arguments that can carry what a person would type: kept as a length.
const REDACTED_ARGS: &[&str] = &["text", "value"];

/// Error text kept per entry; the full error is in the call's log.
const MAX_ERROR: usize = 500;

pub struct Journal {
    /// The server's start, `2026-11-07T16-40-12` (UTC): `forge verify --run`
    /// takes it as the start of the run.
    started: String,
    dir: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    file: Option<File>,
    seq: u64,
    /// The first failure to open the file, so it is reported once.
    failed: bool,
}

impl Journal {
    /// A journal for a server rooted at `root`. Nothing is written yet.
    pub fn new(root: &Path) -> Self {
        let started = timestamp();
        Self {
            dir: root.join(".forge/runs").join(format!("{started}-mcp")),
            started,
            state: Mutex::new(State::default()),
        }
    }

    pub fn started(&self) -> &str {
        &self.started
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("journal.jsonl")
    }

    /// Appends one call. A journal that can't be written never fails the
    /// tool: it says so once on stderr (the transport is stdout).
    pub fn record(
        &self,
        start: &Value,
        tool: &str,
        args: &Map<String, Value>,
        result: &Value,
        ms: u64,
    ) {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.failed {
            return;
        }
        if st.file.is_none() {
            let opened = std::fs::create_dir_all(&self.dir).and_then(|_| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.path())
            });
            match opened {
                Ok(mut f) => {
                    let _ = writeln!(f, "{start}");
                    st.file = Some(f);
                }
                Err(e) => {
                    eprintln!(
                        "forge mcp: run journal off ({}: {e})",
                        self.path().display()
                    );
                    st.failed = true;
                    return;
                }
            }
        }
        st.seq += 1;
        let line = entry(st.seq, tool, args, result, ms);
        if let Some(f) = st.file.as_mut() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

/// The first line: who started the server, with which allowances.
pub fn start_line(opts: &crate::Options) -> Value {
    json!({
        "schema": SCHEMA,
        "event": "start",
        "at": timestamp(),
        "forge_version": env!("CARGO_PKG_VERSION"),
        "root": opts.root.display().to_string(),
        "allow_install": opts.allow_install,
        "no_device": opts.no_device,
        "bundle_ids": opts.bundle_ids,
        "udid": opts.udid,
    })
}

/// One tool call: the arguments (typed text as its length), the outcome and
/// the files the result points at.
pub fn entry(seq: u64, tool: &str, args: &Map<String, Value>, result: &Value, ms: u64) -> Value {
    let args: Map<String, Value> = args
        .iter()
        .map(|(k, v)| match (REDACTED_ARGS.contains(&k.as_str()), v) {
            (true, Value::String(s)) => (k.clone(), json!({ "chars": s.chars().count() })),
            _ => (k.clone(), v.clone()),
        })
        .collect();
    let mut out = json!({
        "event": "call",
        "seq": seq,
        "at": timestamp(),
        "tool": tool,
        "args": args,
        "ok": result["ok"] == json!(true),
        "stage": result["stage"],
        "duration_ms": ms,
    });
    let obj = out.as_object_mut().expect("object");
    if let Some(cli) = result["cli"].as_str() {
        obj.insert("cli".into(), json!(cli));
    }
    if let Some(e) = result["error"].as_str() {
        let cut: String = e.chars().take(MAX_ERROR).collect();
        obj.insert("error".into(), json!(cut));
    }
    if result["needs_person"] == json!(true) {
        obj.insert("needs_person".into(), json!(true));
    }
    if let Some(hints) = result["hints"].as_array() {
        // Forge's own fixed sentences (diagnose.rs), never app output.
        let keys: Vec<&Value> = hints.iter().filter_map(|h| h.get("problem")).collect();
        if !keys.is_empty() {
            obj.insert("hints".into(), json!(keys));
        }
    }
    // Where the details are: the command's log and any evidence bundle.
    for (key, val) in [
        ("log", &result["log"]),
        ("evidence", &result["data"]["evidence"]),
    ] {
        if let Some(p) = val.as_str() {
            obj.insert(key.into(), json!(p));
        }
    }
    out
}

/// `2026-11-07T16-40-12` in UTC, the same stamp as `forge`'s logs and runs
/// (forge-cli `report::timestamp`).
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_is_kept_as_its_length_only() {
        let args =
            json!({"action": "type", "id": "bill.amount", "text": "secret 42", "value": "£50"});
        let result = json!({
            "ok": false, "stage": "ui", "cli": "forge --json ui run step.toml",
            "error": "x".repeat(2000), "needs_person": true,
            "hints": [{"stage": "signing", "problem": "The profile expired.", "fix": "f"}],
            "log": "app/.forge/logs/a.log", "data": {"evidence": "app/.forge/runs/b/evidence.json"}
        });
        let e = entry(3, "forge_ui_act", args.as_object().unwrap(), &result, 1200);
        let text = e.to_string();
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("£50"), "{text}");
        assert_eq!(e["args"]["text"]["chars"], 9);
        assert_eq!(e["args"]["value"]["chars"], 3);
        assert_eq!(e["args"]["id"], "bill.amount");
        assert_eq!(e["seq"], 3);
        assert_eq!(e["ok"], false);
        assert_eq!(e["error"].as_str().unwrap().len(), MAX_ERROR);
        assert_eq!(e["needs_person"], true);
        assert_eq!(e["hints"][0], "The profile expired.");
        assert_eq!(e["evidence"], "app/.forge/runs/b/evidence.json");
    }

    #[test]
    fn writes_a_start_line_then_one_line_per_call() {
        let tmp = tempfile::tempdir().unwrap();
        let j = Journal::new(tmp.path());
        assert!(!j.path().exists(), "nothing written before the first call");
        let start = json!({"schema": SCHEMA, "event": "start"});
        let ok = json!({"ok": true, "stage": null});
        j.record(&start, "forge_doctor", &Map::new(), &ok, 5);
        j.record(&start, "forge_check", &Map::new(), &ok, 7);
        let text = std::fs::read_to_string(j.path()).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["event"], "start");
        assert_eq!(lines[2]["tool"], "forge_check");
        assert_eq!(lines[2]["seq"], 2);
    }

    #[test]
    fn timestamp_has_the_run_id_shape() {
        let t = timestamp();
        assert_eq!(t.len(), 19, "{t}");
        assert_eq!(&t[10..11], "T");
    }
}
