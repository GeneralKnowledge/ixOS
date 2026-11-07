//! Runs `forge <command> --json` as a child process and turns what it prints
//! into a `forge.result/1` object, whatever happened: a normal result, a
//! crash, a command this build does not have, a timeout or a cancelled call.
//!
//! The child gets its own process group, so a timeout or a cancelled call
//! also stops what it started (xtool, cargo, swift), not just `forge`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

/// How much of the child's stderr is kept for a result that has none of its own.
const STDERR_TAIL: usize = 4000;

/// What a tool asks the runner for.
#[derive(Debug, Clone)]
pub struct Invocation {
    /// The arguments after `forge`, without `--json`.
    pub args: Vec<String>,
    /// The working directory: the app, so the result's `app` is filled in.
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Stage to report when the child is stopped or prints no result.
    pub stage: &'static str,
}

impl Invocation {
    pub fn new(args: &[&str], cwd: &Path, timeout: Duration, stage: &'static str) -> Self {
        Self {
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: cwd.to_path_buf(),
            timeout,
            stage,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn opt(self, flag: &str, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => self.arg(flag).arg(v),
            None => self,
        }
    }

    /// The command line a person would type.
    pub fn display(&self) -> String {
        let mut out = String::from("forge --json");
        for a in &self.args {
            out.push(' ');
            if a.is_empty() || a.contains(char::is_whitespace) {
                out.push_str(&format!("'{a}'"));
            } else {
                out.push_str(a);
            }
        }
        out
    }
}

/// Runs `forge` (the binary at `forge`) with `--json` and returns its
/// `forge.result/1` object, real or made up for a run that never produced one.
pub async fn run(
    forge: &Path,
    workspace: &Path,
    inv: &Invocation,
    cancel: &CancellationToken,
) -> Value {
    let started = Instant::now();
    let mut cmd = tokio::process::Command::new(forge);
    cmd.arg("--json")
        .args(&inv.args)
        .current_dir(&inv.cwd)
        // Apps outside the checkout still find Forge's scripts and templates.
        .env("FORGE_WORKSPACE", workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return made_up(
                inv,
                "forge",
                &format!("could not start {}: {e}", forge.display()),
                "",
                started,
            )
        }
    };
    let pid = child.id();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let read_out = async {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        buf
    };
    let read_err = async {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    };
    let finished = async {
        let (out, err, status) = tokio::join!(read_out, read_err, child.wait());
        (out, err, status)
    };

    let outcome = tokio::select! {
        done = finished => Some(done),
        _ = tokio::time::sleep(inv.timeout) => None,
        _ = cancel.cancelled() => None,
    };
    let Some((out, err, status)) = outcome else {
        kill_group(pid);
        let cancelled = cancel.is_cancelled();
        let why = if cancelled {
            "cancelled by the client; the command and everything it started were stopped"
                .to_string()
        } else {
            format!(
                "no result after {} s; the command and everything it started were stopped",
                inv.timeout.as_secs()
            )
        };
        let mut result = made_up(inv, inv.stage, &why, "", started);
        if !cancelled && inv.stage == "device" {
            let obj = result.as_object_mut().expect("object");
            obj.insert("needs_person".into(), json!(true));
            obj.insert(
                "hints".into(),
                json!([{
                    "stage": "device",
                    "problem": "The iPhone stopped answering: it may have dropped off USB, locked, or usbmuxd may be stuck.",
                    "fix": "Stop and ask the person to replug and unlock the iPhone (`sudo systemctl restart usbmuxd` if it still hangs). Do not retry in a loop."
                }]),
            );
        }
        return result;
    };

    let stderr_text = String::from_utf8_lossy(&err);
    let stdout_text = String::from_utf8_lossy(&out);
    let parsed = stdout_text
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .and_then(|l| serde_json::from_str::<Value>(l).ok())
        .or_else(|| serde_json::from_str::<Value>(stdout_text.trim()).ok())
        .filter(Value::is_object);
    let result = match parsed {
        Some(mut value) => {
            // `forge doctor --json` predates the result contract: no `ok`.
            let obj = value.as_object_mut().expect("object");
            if !obj.contains_key("ok") {
                obj.insert(
                    "ok".into(),
                    json!(status.as_ref().is_ok_and(|s| s.success())),
                );
            }
            value
        }
        None => {
            let code = status.as_ref().ok().and_then(|s| s.code());
            let message = if code == Some(2) && stderr_text.contains("unrecognized subcommand") {
                format!(
                    "this forge build has no `{}` command. {}",
                    inv.args.first().map(String::as_str).unwrap_or(""),
                    "Update Forge (or merge the PR that adds it) and restart `forge mcp`."
                )
            } else {
                format!(
                    "`{}` printed no result (exit {})",
                    inv.display(),
                    code.map_or("by signal".into(), |c| c.to_string())
                )
            };
            let stage = if code == Some(2) { "usage" } else { inv.stage };
            made_up(inv, stage, &message, &stderr_text, started)
        }
    };
    result
}

/// A `forge.result/1` object for a run that never printed one.
fn made_up(inv: &Invocation, stage: &str, error: &str, stderr: &str, started: Instant) -> Value {
    let mut obj = Map::new();
    obj.insert("schema".into(), json!("forge.result/1"));
    obj.insert("command".into(), json!(command_name(&inv.args)));
    obj.insert("ok".into(), json!(false));
    obj.insert("stage".into(), json!(stage));
    obj.insert(
        "duration_ms".into(),
        json!(started.elapsed().as_millis() as u64),
    );
    obj.insert("error".into(), json!(error));
    obj.insert("diagnostics".into(), json!([]));
    obj.insert("hints".into(), json!([]));
    if !stderr.trim().is_empty() {
        obj.insert("stderr_tail".into(), json!(tail(stderr, STDERR_TAIL)));
    }
    Value::Object(obj)
}

/// `run ios` from `["run", "ios", "--project", …]`: the words before the first flag.
fn command_name(args: &[String]) -> String {
    args.iter()
        .take_while(|a| !a.starts_with('-'))
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The last `max` bytes of `text`, cut at a line start.
pub fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let cut = &text[start..];
    match cut.find('\n') {
        Some(i) => cut[i + 1..].to_string(),
        None => cut.to_string(),
    }
}

fn kill_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        // SAFETY: sends SIGKILL to the process group this module created for
        // the child (process_group(0) makes the child its leader).
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_and_names() {
        let inv = Invocation::new(
            &["run", "ios"],
            Path::new("."),
            Duration::from_secs(1),
            "forge",
        )
        .opt("--project", Some("my app"))
        .opt("--udid", None::<String>);
        assert_eq!(inv.display(), "forge --json run ios --project 'my app'");
        assert_eq!(command_name(&inv.args), "run ios");
        assert_eq!(tail("one\ntwo\nthree\n", 8), "three\n");
        assert_eq!(tail("short", 100), "short");
    }
}
