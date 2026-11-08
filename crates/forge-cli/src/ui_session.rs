//! `forge ui serve|start|stop|status`: one WebDriverAgent runner kept
//! running between `forge ui` commands, so each command skips the runner
//! start (about 2 s on Cati). The session itself is in
//! `forge_uitest::session`; this file wires it to the command line.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use forge_uitest::phone::{find_runner, Phone};
use forge_uitest::session::{self, ServeOptions, SessionInfo, State};

use crate::report;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Set from SIGINT, SIGTERM and SIGHUP; `serve` checks it four times a second.
static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn catch_stop_signals() {
    #[cfg(unix)]
    // SAFETY: the handler only stores to an atomic, which is
    // async-signal-safe.
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
    }
}

fn idle(minutes: u64) -> Option<Duration> {
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

/// `forge ui serve`: hold the runner in this process until Ctrl-C,
/// `forge ui stop`, the idle time, or `--exit-with`'s process exiting.
pub fn serve(
    udid: Option<&str>,
    idle_minutes: u64,
    exit_with: Option<u32>,
    out: &mut dyn Write,
) -> Result<(), String> {
    catch_stop_signals();
    let phone = Phone::open(udid).map_err(err)?;
    report::set_device(&phone.udid, None);
    if !phone.ddi_mounted().map_err(err)? {
        return Err("the Developer Disk Image is not mounted (the phone restarted?): run `forge ui setup --ddi <folder>`".into());
    }
    let apps = phone.user_apps().map_err(err)?;
    let runner_id = find_runner(&apps).ok_or(crate::ui::NO_RUNNER)?.to_owned();
    let opts = ServeOptions {
        idle: idle(idle_minutes),
        exit_with,
        ..ServeOptions::default()
    };
    let reason = session::serve(
        &phone,
        &runner_id,
        apps,
        &opts,
        &|| STOP.load(Ordering::SeqCst),
        out,
    )
    .map_err(err)?;
    report::set_data(serde_json::json!({ "stopped": reason.to_string() }));
    Ok(())
}

/// `forge ui start`: run `forge ui serve` in the background and wait until
/// its runner is ready.
pub fn start(
    udid: Option<&str>,
    idle_minutes: u64,
    exit_with: Option<u32>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let dir = session::dir();
    // Only to learn which phone; the session opens it again.
    let udid = Phone::open(udid).map_err(err)?.udid;
    report::set_device(&udid, None);
    if let Some(info) = session::read_in(&dir, &udid) {
        if session::pid_alive(info.pid) {
            session::touch_in(&dir, &udid);
            report::set_data(describe(&dir, &info));
            return writeln!(
                out,
                "A UI session is already running for {udid} (process {}, {})",
                info.pid,
                state_word(info.state)
            )
            .map_err(err);
        }
        session::remove_in(&dir, &udid, info.pid);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let log_path = session::log_path_in(&dir, &udid);
    let log =
        std::fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let log_err = log.try_clone().map_err(err)?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the forge binary: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["ui", "serve", "--udid", &udid, "--idle-minutes"])
        .arg(idle_minutes.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(log_err);
    if let Some(pid) = exit_with {
        cmd.arg("--exit-with").arg(pid.to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own session: no terminal hang-up, and a caller that kills its
        // process group (forge mcp on a timeout) doesn't take it along.
        // SAFETY: setsid is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start `forge ui serve`: {e}"))?;
    let pid = child.id();
    writeln!(out, "Starting a UI session for {udid} (process {pid})…").map_err(err)?;

    // A local file and the child's exit status; the phone isn't asked.
    let deadline = Instant::now() + session::WAIT_FOR_READY;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "the UI session stopped ({status}):\n{}",
                tail(&log_path, 2000)
            ));
        }
        if let Some(info) = session::read_in(&dir, &udid).filter(|i| i.pid == pid) {
            if info.state == State::Ready {
                report::set_data(describe(&dir, &info));
                return writeln!(
                    out,
                    "WebDriverAgent {} is ready. `forge ui` commands use this runner until `forge ui stop`{}. Log: {}",
                    info.wda_version.as_deref().unwrap_or("(unknown version)"),
                    match idle(idle_minutes) {
                        Some(d) => format!(" or {} min without a command", d.as_secs() / 60),
                        None => String::new(),
                    },
                    log_path.display()
                )
                .map_err(err);
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            session::remove_in(&dir, &udid, pid);
            return Err(format!(
                "the UI session's runner was not ready after {} s; stopped it:\n{}",
                session::WAIT_FOR_READY.as_secs(),
                tail(&log_path, 2000)
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// `forge ui stop`: end the session(s) and their runners.
pub fn stop(udid: Option<&str>, out: &mut dyn Write) -> Result<(), String> {
    let dir = session::dir();
    let sessions: Vec<SessionInfo> = session::list_in(&dir)
        .into_iter()
        .filter(|s| udid.is_none_or(|u| u == s.udid))
        .collect();
    if sessions.is_empty() {
        report::set_data(serde_json::json!({ "stopped": [] }));
        return writeln!(out, "No UI session is running").map_err(err);
    }
    let mut stopped = Vec::new();
    for info in sessions {
        if !session::pid_alive(info.pid) {
            session::remove_in(&dir, &info.udid, info.pid);
            writeln!(out, "{}: the session had already ended", info.udid).map_err(err)?;
            continue;
        }
        let how = end(info.pid);
        session::remove_in(&dir, &info.udid, info.pid);
        writeln!(
            out,
            "{}: stopped the UI session (process {}{how})",
            info.udid, info.pid
        )
        .map_err(err)?;
        stopped.push(info.udid);
    }
    report::set_data(serde_json::json!({ "stopped": stopped }));
    Ok(())
}

/// SIGTERM, then SIGKILL if it hasn't gone after 20 s (it shuts WDA down
/// first, which takes at most a few seconds).
fn end(pid: u32) -> &'static str {
    #[cfg(unix)]
    {
        let Ok(p) = libc::pid_t::try_from(pid) else {
            return "";
        };
        // SAFETY: plain signals to a process we found in our own state file.
        unsafe { libc::kill(p, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if !session::pid_alive(pid) {
                return "";
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        unsafe { libc::kill(p, libc::SIGKILL) };
        ", killed after 20 s"
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        ""
    }
}

/// `forge ui status`: the sessions on this computer. Doesn't touch the
/// phone or count as using a session.
pub fn status(udid: Option<&str>, out: &mut dyn Write) -> Result<(), String> {
    let dir = session::dir();
    let sessions: Vec<SessionInfo> = session::list_in(&dir)
        .into_iter()
        .filter(|s| udid.is_none_or(|u| u == s.udid))
        .collect();
    let list: Vec<serde_json::Value> = sessions.iter().map(|s| describe(&dir, s)).collect();
    report::set_data(serde_json::json!({ "sessions": list }));
    if sessions.is_empty() {
        return writeln!(
            out,
            "No UI session is running: each `forge ui` command starts its own runner. `forge ui start` keeps one running"
        )
        .map_err(err);
    }
    for (info, d) in sessions.iter().zip(&list) {
        if !session::pid_alive(info.pid) {
            writeln!(
                out,
                "{}: ended (process {} is gone; the next command clears it)",
                info.udid, info.pid
            )
            .map_err(err)?;
            continue;
        }
        writeln!(
            out,
            "{}: {} (process {}), WebDriverAgent {}, up {}, last used {} ago, {} restart(s), {}",
            info.udid,
            state_word(info.state),
            info.pid,
            info.wda_version.as_deref().unwrap_or("(unknown version)"),
            minutes(d["up_secs"].as_u64().unwrap_or(0)),
            minutes(d["idle_secs"].as_u64().unwrap_or(0)),
            info.restarts,
            match info.idle_secs {
                0 => "no idle stop".to_string(),
                s => format!("stops after {} idle", minutes(s)),
            },
        )
        .map_err(err)?;
        writeln!(
            out,
            "  log: {}",
            session::log_path_in(&dir, &info.udid).display()
        )
        .map_err(err)?;
    }
    Ok(())
}

fn state_word(state: State) -> &'static str {
    match state {
        State::Starting => "starting",
        State::Ready => "ready",
        State::Restarting => "restarting its runner",
    }
}

fn minutes(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min", secs / 60)
    }
}

/// A session as `--json` reports it.
fn describe(dir: &Path, info: &SessionInfo) -> serde_json::Value {
    let now = std::time::SystemTime::now();
    let idle = session::last_used_in(dir, &info.udid)
        .and_then(|t| now.duration_since(t).ok())
        .map_or(0, |d| d.as_secs());
    serde_json::json!({
        "udid": info.udid,
        "pid": info.pid,
        "alive": session::pid_alive(info.pid),
        "state": info.state,
        "runner": info.runner,
        "wda_version": info.wda_version,
        "up_secs": session::now_secs().saturating_sub(info.started),
        "idle_secs": idle,
        "idle_limit_secs": info.idle_secs,
        "restarts": info.restarts,
        "exit_with": info.exit_with,
        "log": session::log_path_in(dir, &info.udid),
    })
}

/// The end of a log file, for an error.
fn tail(path: &Path, max: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let start = text.len().saturating_sub(max);
    let start = (start..=text.len())
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(text.len());
    text[start..].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_idle_minutes_means_never() {
        assert_eq!(idle(0), None);
        assert_eq!(idle(30), Some(Duration::from_secs(1800)));
        assert_eq!(minutes(59), "59 s");
        assert_eq!(minutes(1800), "30 min");
    }

    #[test]
    fn tails_on_a_character_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("log");
        std::fs::write(&p, "ééé\n").unwrap();
        assert_eq!(tail(&p, 3), "é");
        assert_eq!(tail(&dir.path().join("none"), 10), "");
    }

    #[test]
    fn status_and_stop_with_no_sessions() {
        let dir = tempfile::tempdir().unwrap();
        // describe() of a dead session says so without touching a phone.
        let info = SessionInfo {
            schema: session::SCHEMA.into(),
            pid: u32::MAX,
            udid: "u".into(),
            runner: "r".into(),
            state: State::Ready,
            started: session::now_secs() - 120,
            restarts: 1,
            apps: vec![],
            wda_version: None,
            idle_secs: 0,
            exit_with: Some(1),
        };
        session::write_in(dir.path(), &info).unwrap();
        let d = describe(dir.path(), &info);
        assert_eq!(d["alive"], false);
        assert_eq!(d["state"], "ready");
        assert!(d["up_secs"].as_u64().unwrap() >= 120);
    }
}
