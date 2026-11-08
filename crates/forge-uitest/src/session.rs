//! A long-lived UI session: one WebDriverAgent runner kept running between
//! `forge ui` commands.
//!
//! Without a session every `forge ui` command starts the runner through
//! `testmanagerd` and the phone tears it down when `forge` exits: about 2 s
//! per command on Cati, most of a short test. An agent that explores an app
//! makes hundreds of calls. `forge ui serve` (or `forge ui start`, which runs
//! it in the background, or `forge mcp`, which starts one for its UI tools)
//! holds the runner instead, and writes a small state file. Other `forge ui`
//! commands find that file, check that the session's process is alive and
//! that WebDriverAgent answers, and talk to the same runner directly over
//! usbmuxd; they skip the runner start, the Developer Disk Image check and
//! the app list.
//!
//! The session stops on `forge ui stop`, Ctrl-C, after a period with no
//! command (`--idle-minutes`, 30 by default), or when the process it was
//! started for exits (`--exit-with <pid>`, which `forge mcp` uses). If
//! WebDriverAgent stops answering it restarts the runner, a bounded number
//! of times.
//!
//! The state file holds no screen content: process id, phone, runner, state,
//! and the bundle ids of installed apps.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::phone::{Phone, Runner};
use crate::{Result, UiError};

/// `schema` of the state file.
pub const SCHEMA: &str = "forge.ui-session/1";

/// How long a session waits for a command before it stops by itself.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(30 * 60);

/// How often the session checks its runner, its idle time and its parent.
pub const TICK: Duration = Duration::from_secs(10);

/// Failed `/status` checks in a row before the runner is restarted.
pub const FAILS_BEFORE_RESTART: u32 = 2;

/// Runner restarts in a row that may fail before the session gives up.
pub const MAX_FAILED_RESTARTS: u32 = 3;

/// How long a command waits for a session that is starting or restarting
/// its runner, before it gives up and says so.
pub const WAIT_FOR_READY: Duration = Duration::from_secs(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Starting the runner for the first time.
    Starting,
    /// WebDriverAgent answers; commands may use it.
    Ready,
    /// WebDriverAgent stopped answering; starting the runner again.
    Restarting,
}

/// What the session writes about itself, one file per phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub schema: String,
    pub pid: u32,
    pub udid: String,
    pub runner: String,
    pub state: State,
    /// Unix seconds.
    pub started: u64,
    /// Runner restarts so far.
    pub restarts: u32,
    /// Bundle ids of the apps installed when the session started; commands
    /// list them again from the phone when an app isn't among them.
    pub apps: Vec<String>,
    pub wda_version: Option<String>,
    /// Seconds without a command before the session stops; 0 = never.
    pub idle_secs: u64,
    /// The process whose exit stops the session.
    pub exit_with: Option<u32>,
}

/// Where session files live: `$FORGE_UI_SESSION_DIR`, else
/// `$XDG_RUNTIME_DIR/forge/ui-session` (cleared at logout), else
/// `~/.cache/forge/ui-session`.
pub fn dir() -> PathBuf {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = env("FORGE_UI_SESSION_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(run) = env("XDG_RUNTIME_DIR") {
        return Path::new(&run).join("forge/ui-session");
    }
    let home = env("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".cache/forge/ui-session")
}

/// The state file for one phone.
pub fn path_in(dir: &Path, udid: &str) -> PathBuf {
    dir.join(format!("{udid}.json"))
}

/// The session's log (what `forge ui start` sends its output to).
pub fn log_path_in(dir: &Path, udid: &str) -> PathBuf {
    dir.join(format!("{udid}.log"))
}

/// The session file for a phone, if there is one and it parses.
pub fn read_in(dir: &Path, udid: &str) -> Option<SessionInfo> {
    let text = std::fs::read_to_string(path_in(dir, udid)).ok()?;
    serde_json::from_str::<SessionInfo>(&text)
        .ok()
        .filter(|s| s.schema == SCHEMA)
}

/// Every session file in `dir`, live or not.
pub fn list_in(dir: &Path) -> Vec<SessionInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<SessionInfo> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let udid = name.strip_suffix(".json")?.to_owned();
            read_in(dir, &udid)
        })
        .collect();
    out.sort_by(|a, b| a.udid.cmp(&b.udid));
    out
}

/// Writes the file whole, through a rename, so a reader never sees half.
pub fn write_in(dir: &Path, info: &SessionInfo) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = path_in(dir, &info.udid);
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(info).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// Removes a session's file, but only if it still belongs to `pid`.
pub fn remove_in(dir: &Path, udid: &str, pid: u32) {
    if read_in(dir, udid).is_none_or(|s| s.pid == pid) {
        let _ = std::fs::remove_file(path_in(dir, udid));
    }
}

/// Marks the session as used now (the file's modification time), which
/// postpones its idle stop.
pub fn touch_in(dir: &Path, udid: &str) {
    if let Ok(file) = std::fs::File::options()
        .append(true)
        .open(path_in(dir, udid))
    {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// When a command last used the session.
pub fn last_used_in(dir: &Path, udid: &str) -> Option<SystemTime> {
    std::fs::metadata(path_in(dir, udid))
        .and_then(|m| m.modified())
        .ok()
}

/// Whether a process with this id exists.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if pid <= 0 {
            return false;
        }
        // Signal 0 checks without sending; EPERM still means it exists.
        // SAFETY: kill with signal 0 has no effect on the target.
        let rc = unsafe { libc::kill(pid, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Unix seconds now.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Why a session stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// `forge ui stop`, Ctrl-C or SIGTERM.
    Asked,
    /// No command for the idle time.
    Idle(Duration),
    /// The process it was started for exited.
    ParentGone(u32),
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::Asked => write!(f, "asked to stop"),
            StopReason::Idle(d) => write!(f, "no command for {} min", d.as_secs() / 60),
            StopReason::ParentGone(pid) => write!(f, "process {pid} it was started for exited"),
        }
    }
}

/// Whether the session should stop now, apart from being asked to.
/// `idle` of `None` means never stop for idleness.
pub fn should_stop(
    now: SystemTime,
    last_used: Option<SystemTime>,
    idle: Option<Duration>,
    parent: Option<(u32, bool)>,
) -> Option<StopReason> {
    if let Some((pid, false)) = parent {
        return Some(StopReason::ParentGone(pid));
    }
    let idle = idle?;
    let since = now.duration_since(last_used?).unwrap_or_default();
    (since >= idle).then_some(StopReason::Idle(idle))
}

/// Counts failed health checks and restarts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Health {
    failed_checks: u32,
    failed_restarts: u32,
}

/// What to do after a health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthAction {
    Keep,
    Restart,
    GiveUp,
}

impl Health {
    /// Records one `/status` check.
    pub fn checked(&mut self, answered: bool) -> HealthAction {
        if answered {
            self.failed_checks = 0;
            return HealthAction::Keep;
        }
        self.failed_checks += 1;
        if self.failed_checks < FAILS_BEFORE_RESTART {
            HealthAction::Keep
        } else if self.failed_restarts >= MAX_FAILED_RESTARTS {
            HealthAction::GiveUp
        } else {
            HealthAction::Restart
        }
    }

    /// Records a restart attempt.
    pub fn restarted(&mut self, ok: bool) {
        self.failed_checks = 0;
        if ok {
            self.failed_restarts = 0;
        } else {
            self.failed_restarts += 1;
        }
    }
}

/// How `serve` behaves.
#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub dir: PathBuf,
    /// `None`: never stop for idleness.
    pub idle: Option<Duration>,
    pub exit_with: Option<u32>,
    pub tick: Duration,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            dir: dir(),
            idle: Some(DEFAULT_IDLE),
            exit_with: None,
            tick: TICK,
        }
    }
}

/// Holds the runner until a stop condition; returns why it stopped.
/// `stop_asked` is checked every quarter second (the CLI sets it from
/// SIGINT/SIGTERM). The state file is removed on every way out.
pub fn serve(
    phone: &Phone,
    runner_id: &str,
    apps: Vec<String>,
    opts: &ServeOptions,
    stop_asked: &dyn Fn() -> bool,
    log: &mut dyn Write,
) -> Result<StopReason> {
    let pid = std::process::id();
    let mut info = SessionInfo {
        schema: SCHEMA.into(),
        pid,
        udid: phone.udid.clone(),
        runner: runner_id.into(),
        state: State::Starting,
        started: now_secs(),
        restarts: 0,
        apps,
        wda_version: None,
        idle_secs: opts.idle.map_or(0, |d| d.as_secs()),
        exit_with: opts.exit_with,
    };
    if let Some(other) = read_in(&opts.dir, &phone.udid) {
        if other.pid != pid && pid_alive(other.pid) {
            return Err(UiError::Device(format!(
                "a UI session for this phone is already running (process {}); `forge ui stop` ends it",
                other.pid
            )));
        }
    }
    write_in(&opts.dir, &info)?;
    let result = hold(phone, &mut info, opts, stop_asked, log);
    remove_in(&opts.dir, &phone.udid, pid);
    result
}

fn hold(
    phone: &Phone,
    info: &mut SessionInfo,
    opts: &ServeOptions,
    stop_asked: &dyn Fn() -> bool,
    log: &mut dyn Write,
) -> Result<StopReason> {
    let say = |log: &mut dyn Write, text: &str| {
        let _ = writeln!(log, "[{}] {text}", now_secs());
        let _ = log.flush();
    };
    say(log, &format!("Starting WebDriverAgent ({})", info.runner));
    let started = Instant::now();
    let mut runner: Option<Runner> = Some(phone.start_runner(&info.runner)?);
    info.wda_version = phone.wda_version();
    info.state = State::Ready;
    write_in(&opts.dir, info)?;
    say(
        log,
        &format!(
            "WebDriverAgent {} ready in {:.1} s; serving phone {}",
            info.wda_version.as_deref().unwrap_or("(unknown version)"),
            started.elapsed().as_secs_f64(),
            info.udid
        ),
    );

    let mut health = Health::default();
    loop {
        let tick_end = Instant::now() + opts.tick;
        while Instant::now() < tick_end {
            if stop_asked() {
                return Ok(finish(phone, runner, StopReason::Asked, log));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let parent = opts.exit_with.map(|p| (p, pid_alive(p)));
        let last_used = last_used_in(&opts.dir, &info.udid);
        if let Some(reason) = should_stop(SystemTime::now(), last_used, opts.idle, parent) {
            return Ok(finish(phone, runner, reason, log));
        }
        match health.checked(phone.wda_answers()) {
            HealthAction::Keep => {}
            HealthAction::GiveUp => {
                drop(runner);
                say(log, "WebDriverAgent keeps failing to start; stopping");
                return Err(UiError::Device(format!(
                    "the runner failed to restart {MAX_FAILED_RESTARTS} times in a row"
                )));
            }
            HealthAction::Restart => {
                say(
                    log,
                    "WebDriverAgent stopped answering; restarting the runner",
                );
                info.state = State::Restarting;
                write_in(&opts.dir, info)?;
                // The old XCTest session must end before a new one starts.
                drop(runner.take());
                match phone.start_runner(&info.runner) {
                    Ok(r) => {
                        runner = Some(r);
                        health.restarted(true);
                        info.restarts += 1;
                        info.state = State::Ready;
                        info.wda_version = phone.wda_version().or(info.wda_version.take());
                        write_in(&opts.dir, info)?;
                        say(log, "WebDriverAgent ready again");
                    }
                    Err(e) => {
                        health.restarted(false);
                        say(log, &format!("restart failed: {e}"));
                    }
                }
            }
        }
    }
}

fn finish(
    phone: &Phone,
    runner: Option<Runner>,
    reason: StopReason,
    log: &mut dyn Write,
) -> StopReason {
    // Ask WDA to stop at once, rather than when the phone notices the
    // XCTest session is gone; the next command's runner then starts clean.
    if runner.is_some() {
        phone.shutdown_wda();
    }
    drop(runner);
    let _ = writeln!(log, "[{}] Stopped: {reason}", now_secs());
    let _ = log.flush();
    reason
}

/// What a command found when it looked for a session.
#[derive(Debug)]
pub enum Attach {
    /// No session for this phone: start a runner of its own.
    None,
    /// A ready session whose WebDriverAgent answered.
    Ready(SessionInfo),
}

/// Looks for a live session for `phone` and checks that its WebDriverAgent
/// answers. Waits (bounded) for a session that is starting or restarting.
/// A session file whose process is gone is removed.
pub fn attach(phone: &Phone) -> Result<Attach> {
    attach_in(&dir(), phone)
}

pub fn attach_in(dir: &Path, phone: &Phone) -> Result<Attach> {
    let deadline = Instant::now() + WAIT_FOR_READY;
    let mut probed_restarts: Option<u32> = None;
    loop {
        let Some(info) = read_in(dir, &phone.udid) else {
            return Ok(Attach::None);
        };
        if !pid_alive(info.pid) {
            remove_in(dir, &phone.udid, info.pid);
            return Ok(Attach::None);
        }
        // Only probe a ready runner, and probe each restart once.
        let fresh = probed_restarts != Some(info.restarts);
        if info.state == State::Ready && fresh {
            if phone.wda_answers() {
                touch_in(dir, &phone.udid);
                return Ok(Attach::Ready(info));
            }
            probed_restarts = Some(info.restarts);
        }
        if Instant::now() >= deadline {
            return Err(UiError::Device(format!(
                "the UI session for this phone (process {}) has no working WebDriverAgent after {} s ({}). \
                 `forge ui status` shows it and its log; `forge ui stop` ends it, after which commands start their own runner",
                info.pid,
                WAIT_FOR_READY.as_secs(),
                match info.state {
                    State::Ready => "it is not answering",
                    State::Starting => "still starting",
                    State::Restarting => "still restarting",
                }
            )));
        }
        // A local file read; the phone is only asked again after the
        // session reports a new runner.
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(udid: &str, pid: u32) -> SessionInfo {
        SessionInfo {
            schema: SCHEMA.into(),
            pid,
            udid: udid.into(),
            runner: "XTL-1.com.facebook.WebDriverAgentRunner.xctrunner".into(),
            state: State::Ready,
            started: 1,
            restarts: 0,
            apps: vec!["XTL-1.org.example.app".into()],
            wda_version: Some("16.15.3".into()),
            idle_secs: 1800,
            exit_with: None,
        }
    }

    #[test]
    fn state_files_round_trip_and_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_in(dir.path(), "a").is_none());
        write_in(dir.path(), &info("b", 2)).unwrap();
        write_in(dir.path(), &info("a", 1)).unwrap();
        std::fs::write(dir.path().join("junk.json"), "{}").unwrap();
        std::fs::write(dir.path().join("a.log"), "log").unwrap();
        assert_eq!(read_in(dir.path(), "a"), Some(info("a", 1)));
        let all: Vec<String> = list_in(dir.path()).into_iter().map(|s| s.udid).collect();
        assert_eq!(all, ["a", "b"]);
        // No temporary files left behind.
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    }

    #[test]
    fn remove_only_takes_the_owners_file() {
        let dir = tempfile::tempdir().unwrap();
        write_in(dir.path(), &info("a", 7)).unwrap();
        remove_in(dir.path(), "a", 8);
        assert!(read_in(dir.path(), "a").is_some());
        remove_in(dir.path(), "a", 7);
        assert!(read_in(dir.path(), "a").is_none());
    }

    #[test]
    fn touch_moves_last_used() {
        let dir = tempfile::tempdir().unwrap();
        write_in(dir.path(), &info("a", 7)).unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .append(true)
            .open(path_in(dir.path(), "a"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let before = last_used_in(dir.path(), "a").unwrap();
        touch_in(dir.path(), "a");
        let after = last_used_in(dir.path(), "a").unwrap();
        assert!(after.duration_since(before).unwrap() >= Duration::from_secs(3000));
        // The content is untouched.
        assert_eq!(read_in(dir.path(), "a"), Some(info("a", 7)));
    }

    #[test]
    fn this_process_is_alive() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
        assert!(!pid_alive(u32::MAX));
    }

    #[test]
    fn stops_when_idle_or_orphaned() {
        let now = SystemTime::now();
        let ago = |s| Some(now - Duration::from_secs(s));
        let idle = Some(Duration::from_secs(600));
        assert_eq!(should_stop(now, ago(10), idle, None), None);
        assert_eq!(
            should_stop(now, ago(601), idle, None),
            Some(StopReason::Idle(Duration::from_secs(600)))
        );
        assert_eq!(should_stop(now, ago(100_000), None, None), None);
        assert_eq!(should_stop(now, ago(10), idle, Some((42, true))), None);
        assert_eq!(
            should_stop(now, ago(10), idle, Some((42, false))),
            Some(StopReason::ParentGone(42))
        );
        // A clock that went backwards isn't idleness.
        assert_eq!(
            should_stop(now, Some(now + Duration::from_secs(60)), idle, None),
            None
        );
    }

    #[test]
    fn restarts_after_two_misses_and_gives_up_after_three_failed_restarts() {
        let mut h = Health::default();
        assert_eq!(h.checked(false), HealthAction::Keep);
        assert_eq!(h.checked(true), HealthAction::Keep);
        assert_eq!(h.checked(false), HealthAction::Keep);
        assert_eq!(h.checked(false), HealthAction::Restart);
        h.restarted(true);
        for _ in 0..MAX_FAILED_RESTARTS {
            assert_eq!(h.checked(false), HealthAction::Keep);
            assert_eq!(h.checked(false), HealthAction::Restart);
            h.restarted(false);
        }
        assert_eq!(h.checked(false), HealthAction::Keep);
        assert_eq!(h.checked(false), HealthAction::GiveUp);
        // A good restart clears the count.
        let mut h = Health::default();
        h.checked(false);
        h.checked(false);
        h.restarted(false);
        h.checked(false);
        h.checked(false);
        h.restarted(true);
        for _ in 0..MAX_FAILED_RESTARTS {
            h.checked(false);
            assert_eq!(h.checked(false), HealthAction::Restart);
            h.restarted(false);
        }
    }

    #[test]
    fn the_session_dir_follows_the_environment() {
        // Only the explicit override is checked: tests share the process
        // environment, so don't change XDG_RUNTIME_DIR or HOME here.
        if std::env::var_os("FORGE_UI_SESSION_DIR").is_none() {
            let d = dir();
            assert!(d.ends_with("ui-session"), "{}", d.display());
        }
    }
}
