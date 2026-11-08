//! `--json`: every command ends with one result object on stdout, for
//! scripts and for the Forge MCP server. The shape is "The result contract"
//! in docs/forge-mcp.md (`forge.result/1`).
//!
//! In JSON mode, stdout carries nothing but that object. Everything a command
//! or its child processes would print (cargo, xtool, `[forge]` progress) goes
//! to stderr instead, and the output of build steps is also copied into a log
//! file the result points at. The exit code follows the failed stage, so a
//! script can branch without parsing: 0 ok, 1 anything unclassified, 2 usage,
//! 10 Rust, 11 Swift, 12 signing, 13 device, 14 test failure.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::diagnose::{Hint, Stage};

pub const SCHEMA: &str = "forge.result/1";

static JSON: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State {
    failure: None,
    data: None,
    device: None,
    log: None,
    saved_stdout: None,
    usage: false,
});

struct State {
    failure: Option<Failure>,
    data: Option<Value>,
    device: Option<Value>,
    log: Option<(PathBuf, File)>,
    /// The real stdout, kept aside while fd 1 points at stderr.
    saved_stdout: Option<File>,
    /// The command was called wrongly (stage `usage`, exit 2).
    usage: bool,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Whether this run prints a JSON result.
pub fn json_mode() -> bool {
    JSON.load(Ordering::Relaxed)
}

/// Switches to JSON mode: from here on, anything written to stdout (by this
/// process or a child it starts) goes to stderr, and only [`finish`] writes
/// to the real stdout.
pub fn enter_json_mode() {
    JSON.store(true, Ordering::Relaxed);
    let _ = std::io::stdout().flush();
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: plain fd duplication on the process's own stdio. dup(1)
        // returns a new descriptor this File then owns; dup2(2, 1) makes fd 1
        // a copy of stderr.
        unsafe {
            let saved = libc::dup(1);
            if saved >= 0 && libc::dup2(2, 1) >= 0 {
                state().saved_stdout = Some(File::from_raw_fd(saved));
            }
        }
    }
}

/// A recognised cause, as the result lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonHint {
    pub stage: &'static str,
    pub problem: String,
    pub fix: String,
}

impl JsonHint {
    pub fn new(stage: Stage, problem: &str, fix: &str) -> Self {
        Self {
            stage: stage.key(),
            problem: problem.to_string(),
            fix: fix.to_string(),
        }
    }

    pub fn from_hint(h: &Hint) -> Self {
        Self::new(h.stage, h.problem, h.fix)
    }
}

struct Failure {
    step: String,
    hints: Vec<JsonHint>,
    output: String,
}

/// Remembers why the command is failing: the step, what [`crate::diagnose`]
/// recognised, and the end of the step's output (parsed for diagnostics).
pub fn record_failure(step: &str, hints: Vec<JsonHint>, output: &str) {
    state().failure = Some(Failure {
        step: step.to_string(),
        hints,
        output: output.to_string(),
    });
}

/// Adds a hint to the failure already recorded (a cause found after the
/// step's own diagnosis).
pub fn add_hint(hint: JsonHint) {
    if let Some(failure) = state().failure.as_mut() {
        failure.hints.push(hint);
    }
}

/// An error in how the command was called: stage `usage`, exit code 2.
pub fn usage_error(message: &str) -> String {
    state().usage = true;
    message.to_string()
}

/// The command's own payload (`data` in the result).
pub fn set_data(data: Value) {
    state().data = Some(data);
}

/// The phone the command used (`device` in the result).
pub fn set_device(udid: &str, name: Option<&str>) {
    state().device = Some(json!({ "udid": udid, "name": name }));
}

/// Opens `<project>/.forge/logs/<time>-<command>.log`; build steps copy their
/// output into it.
pub fn open_log(project: &Path, command: &str) {
    let dir = project.join(".forge").join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let slug: String = command
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = dir.join(format!("{}-{slug}.log", timestamp()));
    if let Ok(file) = File::create(&path) {
        state().log = Some((path, file));
    }
}

/// The open log file's path.
pub fn log_path() -> Option<std::path::PathBuf> {
    state().log.as_ref().map(|(p, _)| p.clone())
}

/// Appends a child's output to the log, if one is open.
pub fn log_bytes(bytes: &[u8]) {
    if let Some((_, file)) = state().log.as_mut() {
        let _ = file.write_all(bytes);
    }
}

/// One compiler or linker message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub message: String,
}

const MAX_DIAGNOSTICS: usize = 50;

/// Errors and warnings from rustc (`error[E0425]: …` then `--> file:line:col`),
/// swiftc and clang (`file:line:col: error: …`) and the linker
/// (`ld.lld: error: …`), with paths under `project` made relative to it.
pub fn parse_diagnostics(output: &str, project: Option<&Path>) -> Vec<Diagnostic> {
    let prefix = project.map(|p| format!("{}/", p.display()));
    let relative = |file: &str| -> String {
        match &prefix {
            Some(pre) if file.starts_with(pre.as_str()) => file[pre.len()..].to_string(),
            _ => file.to_string(),
        }
    };
    let mut out: Vec<Diagnostic> = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    for (i, raw) in lines.iter().enumerate() {
        let line = strip_ansi(raw);
        let line = line.trim_end();
        // rustc: `error[E0425]: msg` / `warning: msg`, location on a later `-->` line.
        if let Some((severity, message)) = rustc_head(line) {
            let location = lines[i + 1..]
                .iter()
                .take(4)
                .map(|l| strip_ansi(l))
                .find_map(|l| l.trim_start().strip_prefix("--> ").map(str::to_string));
            // A bare `error: …` with no code and no location is a tool's
            // summary (SwiftPM's `error: fatalError`), not a rustc diagnostic.
            if location.is_none() && !line.starts_with("error[") && !line.starts_with("warning[") {
                continue;
            }
            let (file, line_no, column) = match location.as_deref().and_then(split_location) {
                Some((f, l, c)) => (Some(relative(f)), Some(l), Some(c)),
                None => (None, None, None),
            };
            out.push(Diagnostic {
                severity,
                source: "rustc",
                file,
                line: line_no,
                column,
                message: message.to_string(),
            });
            continue;
        }
        // swiftc / clang: `/path/File.swift:42:17: error: msg`.
        if let Some((loc, rest)) = split_once_any(line, &[": error: ", ": warning: "]) {
            if let Some((file, l, c)) = split_location(loc) {
                let severity = if rest.0 == ": error: " {
                    "error"
                } else {
                    "warning"
                };
                let source = if file.ends_with(".swift") {
                    "swiftc"
                } else {
                    "clang"
                };
                out.push(Diagnostic {
                    severity,
                    source,
                    file: Some(relative(file)),
                    line: Some(l),
                    column: Some(c),
                    message: rest.1.to_string(),
                });
                continue;
            }
        }
        // The linker: `ld.lld: error: undefined symbol: …`, `ld: error: …`.
        for linker in ["ld64.lld: error: ", "ld.lld: error: ", "ld: error: "] {
            if let Some(message) = line.strip_prefix(linker) {
                out.push(Diagnostic {
                    severity: "error",
                    source: "ld",
                    file: None,
                    line: None,
                    column: None,
                    message: message.to_string(),
                });
                break;
            }
        }
    }
    // Errors first; drop exact repeats (SwiftPM prints some twice).
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert((d.file.clone(), d.line, d.column, d.message.clone())));
    out.sort_by_key(|d| d.severity != "error");
    out.truncate(MAX_DIAGNOSTICS);
    out
}

fn rustc_head(line: &str) -> Option<(&'static str, &str)> {
    let (severity, rest) = match line.strip_prefix("error") {
        Some(r) => ("error", r),
        None => ("warning", line.strip_prefix("warning")?),
    };
    // `error[E0425]: msg` or `error: msg`.
    let rest = match rest.strip_prefix('[') {
        Some(r) => &r[r.find("]")? + 1..],
        None => rest,
    };
    let message = rest.strip_prefix(": ")?;
    // Cargo's summaries, not diagnostics.
    if message.starts_with("could not compile")
        || message.starts_with("aborting due to")
        || message.contains("warnings emitted")
        || message.contains("warning emitted")
        || message.starts_with("unused manifest key")
    {
        return None;
    }
    Some((severity, message))
}

/// `file:line:col` → parts; the file may itself contain colons.
fn split_location(loc: &str) -> Option<(&str, u32, u32)> {
    let (rest, col) = loc.rsplit_once(':')?;
    let (file, line) = rest.rsplit_once(':')?;
    if file.is_empty() {
        return None;
    }
    Some((file, line.parse().ok()?, col.parse().ok()?))
}

fn split_once_any<'a>(
    line: &'a str,
    seps: &[&'static str],
) -> Option<(&'a str, (&'static str, &'a str))> {
    seps.iter()
        .filter_map(|sep| line.find(sep).map(|i| (i, *sep)))
        .min_by_key(|(i, _)| *i)
        .map(|(i, sep)| (&line[..i], (sep, &line[i + sep.len()..])))
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI: ESC [ ... final byte in @..~
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The exit code for a stage key.
pub fn exit_code(stage: &str) -> i32 {
    match stage {
        "ok" => 0,
        "usage" => 2,
        "rust" => 10,
        "swift" => 11,
        "signing" => 12,
        "device" => 13,
        "test" => 14,
        _ => 1,
    }
}

/// Which stage failed: the first recognised cause, else what the compiler
/// output shows, else the step that ran, else `fallback`.
fn failed_stage(
    failure: Option<&Failure>,
    diagnostics: &[Diagnostic],
    fallback: &'static str,
) -> &'static str {
    if let Some(h) = failure.and_then(|f| f.hints.first()) {
        return h.stage;
    }
    if let Some(d) = diagnostics.iter().find(|d| d.severity == "error") {
        return match d.source {
            "rustc" => "rust",
            _ => "swift",
        };
    }
    if let Some(step) = failure.map(|f| f.step.as_str()) {
        if step.contains("build-rust") || step.starts_with("cargo") {
            return "rust";
        }
        if step.contains("xtool dev build") {
            return "swift";
        }
    }
    fallback
}

/// What [`finish`] needs to know about the command.
pub struct Outcome<'a> {
    /// `run ios`, `device logs`, …
    pub command: &'a str,
    pub started: Instant,
    /// The app directory, when the command works on one.
    pub project: Option<&'a Path>,
    /// The stage an unrecognised failure counts as (`test` for `forge test`).
    pub fallback_stage: &'static str,
}

/// Builds the result object.
pub fn result(outcome: &Outcome, res: &Result<(), String>) -> Value {
    let mut st = state();
    let failure = st.failure.take();
    let ok = res.is_ok();
    let diagnostics = match (&failure, ok) {
        (Some(f), false) => parse_diagnostics(&f.output, outcome.project),
        _ => Vec::new(),
    };
    let stage = if ok {
        "ok"
    } else if st.usage {
        "usage"
    } else {
        failed_stage(failure.as_ref(), &diagnostics, outcome.fallback_stage)
    };
    let hints = failure
        .as_ref()
        .map(|f| f.hints.clone())
        .unwrap_or_default();

    let mut obj = Map::new();
    obj.insert("schema".into(), json!(SCHEMA));
    obj.insert("command".into(), json!(outcome.command));
    obj.insert("ok".into(), json!(ok));
    obj.insert("stage".into(), json!(stage));
    obj.insert(
        "duration_ms".into(),
        json!(outcome.started.elapsed().as_millis() as u64),
    );
    if let Err(e) = res {
        obj.insert("error".into(), json!(e));
    }
    obj.insert("diagnostics".into(), json!(diagnostics));
    obj.insert("hints".into(), json!(hints));
    obj.insert("device".into(), st.device.take().unwrap_or(Value::Null));
    obj.insert(
        "app".into(),
        outcome.project.and_then(app_info).unwrap_or(Value::Null),
    );
    obj.insert(
        "log".into(),
        st.log
            .as_ref()
            .map(|(p, _)| json!(p.display().to_string()))
            .unwrap_or(Value::Null),
    );
    obj.insert("data".into(), st.data.take().unwrap_or(Value::Null));
    Value::Object(obj)
}

fn app_info(project: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(project.join("forge.toml")).ok()?;
    let cfg = crate::config::ForgeConfig::parse(&text).ok()?;
    Some(json!({
        "name": cfg.app.name,
        "bundle_id": cfg.app.bundle_id,
        "product": crate::app_config::xtool_product(&cfg),
        "path": project.display().to_string(),
    }))
}

/// Prints the result on the real stdout and returns the exit code.
pub fn finish(outcome: &Outcome, res: Result<(), String>) -> i32 {
    let value = result(outcome, &res);
    let code = exit_code(value["stage"].as_str().unwrap_or(""));
    let _ = std::io::stdout().flush();
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    let mut st = state();
    match st.saved_stdout.as_mut() {
        Some(out) => {
            let _ = writeln!(out, "{text}");
            let _ = out.flush();
        }
        None => println!("{text}"),
    }
    code
}

/// `2026-11-07T16-40-12` in UTC, for log file names and run ids.
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    stamp_at(secs)
}

/// [`timestamp`] for a time in seconds since 1970 (UTC).
pub fn stamp_at(secs: u64) -> String {
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

/// `30s`, `2m`, `500ms`, `1h`, or plain seconds.
pub fn parse_duration(s: &str) -> Result<std::time::Duration, String> {
    let s = s.trim();
    let (num, unit) = match s.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, "s"),
    };
    let n: f64 = num
        .parse()
        .map_err(|_| format!("`{s}` is not a duration (try 30s, 2m, 500ms)"))?;
    let secs = match unit {
        "ms" => n / 1000.0,
        "s" => n,
        "m" => n * 60.0,
        "h" => n * 3600.0,
        _ => return Err(format!("`{s}` is not a duration (try 30s, 2m, 500ms)")),
    };
    if !secs.is_finite() || secs < 0.0 {
        return Err(format!("`{s}` is not a duration"));
    }
    Ok(std::time::Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_swiftc_rustc_and_linker_messages() {
        let out = "\
Compiling AppHost NotesModel.swift
/home/me/MyApp/Sources/AppHost/TimelineView.swift:42:17: error: value of type 'Post' has no member 'likeCount'
/home/me/MyApp/Sources/AppHost/TimelineView.swift:42:17: error: value of type 'Post' has no member 'likeCount'
/home/me/MyApp/Sources/AppHost/Other.swift:3:1: warning: unused variable 'x'
error[E0425]: cannot find value `y` in this scope
  --> src/lib.rs:10:5
   |
error: could not compile `my-app` (lib) due to 1 previous error
ld.lld: error: undefined symbol: uniffi_forge_ffi_fn_x
error: fatalError
";
        let d = parse_diagnostics(out, Some(Path::new("/home/me/MyApp")));
        assert_eq!(d.len(), 4, "{d:#?}");
        assert_eq!(d[0].source, "swiftc");
        assert_eq!(
            d[0].file.as_deref(),
            Some("Sources/AppHost/TimelineView.swift")
        );
        assert_eq!((d[0].line, d[0].column), (Some(42), Some(17)));
        assert_eq!(
            d[0].message,
            "value of type 'Post' has no member 'likeCount'"
        );
        assert_eq!(d[1].source, "rustc");
        assert_eq!(d[1].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(d[1].line, Some(10));
        assert_eq!(d[2].source, "ld");
        // Warnings come after errors.
        assert_eq!(d[3].severity, "warning");
    }

    #[test]
    fn ansi_colours_do_not_hide_rust_errors() {
        let out = "\u{1b}[0m\u{1b}[1m\u{1b}[38;5;9merror[E0308]\u{1b}[0m\u{1b}[0m\u{1b}[1m: mismatched types\u{1b}[0m\n  \u{1b}[0m\u{1b}[0m\u{1b}[1m\u{1b}[38;5;12m--> \u{1b}[0m\u{1b}[0msrc/lib.rs:3:9\n";
        let d = parse_diagnostics(out, None);
        assert_eq!(d.len(), 1, "{d:#?}");
        assert_eq!(d[0].message, "mismatched types");
        assert_eq!(d[0].file.as_deref(), Some("src/lib.rs"));
    }

    #[test]
    fn the_stage_comes_from_hints_then_diagnostics_then_the_step() {
        let f = |step: &str, hints: Vec<JsonHint>| Failure {
            step: step.into(),
            hints,
            output: String::new(),
        };
        let hint = JsonHint::new(Stage::Signing, "p", "f");
        assert_eq!(
            failed_stage(Some(&f("xtool dev run", vec![hint])), &[], "forge"),
            "signing"
        );
        let swift = parse_diagnostics("/a/B.swift:1:2: error: nope", None);
        assert_eq!(
            failed_stage(Some(&f("xtool dev run", vec![])), &swift, "forge"),
            "swift"
        );
        assert_eq!(
            failed_stage(
                Some(&f("./scripts/build-rust-ios.sh", vec![])),
                &[],
                "forge"
            ),
            "rust"
        );
        assert_eq!(failed_stage(None, &[], "test"), "test");
        assert_eq!(exit_code("swift"), 11);
        assert_eq!(exit_code("device"), 13);
        assert_eq!(exit_code("forge"), 1);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s").unwrap().as_secs(), 30);
        assert_eq!(parse_duration("2m").unwrap().as_secs(), 120);
        assert_eq!(parse_duration("500ms").unwrap().as_millis(), 500);
        assert_eq!(parse_duration("5").unwrap().as_secs(), 5);
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn timestamps_sort_and_name_files() {
        let t = timestamp();
        assert_eq!(t.len(), "2026-11-07T16-40-12".len(), "{t}");
        assert!(t.starts_with("20"), "{t}");
    }
}
