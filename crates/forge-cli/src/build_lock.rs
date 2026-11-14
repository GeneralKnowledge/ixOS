//! The build queue: one heavy build at a time on a machine, in the order
//! they arrived, each inside a memory and CPU cap.
//!
//! Several sessions building on one machine at once (the device-host laptop,
//! 2026-11-14) ran it out of memory: Cargo, SwiftPM and xtool each use every
//! core and gigabytes of memory, the machine swapped until `ps` hung, and
//! Remote Control and over-the-air downloads died. So:
//!
//! - **Queue.** `forge build ios`, `forge run ios` (its build part),
//!   `forge publish ios`, `forge queue run -- <command>` and
//!   `scripts/check-ios-xtool.sh` take a ticket in `build-queue/` and wait
//!   until theirs is the oldest live ticket, then take the exclusive
//!   `flock(2)` on `build.lock` (both under `~/.cache/forge/`). A waiter
//!   prints its place in line and who is building; `forge queue status`
//!   lists both. The kernel drops the lock when the holder exits, however it
//!   exits, and tickets of processes that are gone are skipped and removed,
//!   so nothing stale blocks the queue. A hung build keeps the lock: waiters
//!   give up after `FORGE_BUILD_LOCK_TIMEOUT` seconds and name its pid.
//! - **Caps.** Each heavy command runs in a transient systemd scope
//!   (`systemd-run --user --scope`) with `MemoryMax` (no swap) and
//!   `CPUQuota`, so a build that outgrows the machine is killed by the
//!   kernel instead of freezing it, and Cargo gets fewer jobs. Without a
//!   user systemd the build runs uncapped with a warning, unless the machine
//!   is marked a shared build host (`~/.config/forge/build-host.toml`,
//!   written by `scripts/setup-build-host.sh`): then it refuses to build.
//! - **Priority.** Builds run at nice 10 and the lowest best-effort I/O
//!   priority, so interactive work on the same machine comes first.
//!
//! Environment: `FORGE_BUILD_LOCK` (another lock file, or `off`),
//! `FORGE_BUILD_LOCK_TIMEOUT` (seconds, default 3600),
//! `FORGE_BUILD_LOCK_NOTE` (who is building, shown to waiters),
//! `FORGE_BUILD_LOCK_HELD` (set for the children of a queued build, so they
//! don't queue behind their own parent), `FORGE_BUILD_MEMORY_MAX` (GiB, or
//! `off`), `FORGE_BUILD_JOBS`, `FORGE_BUILD_NICE=0`.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::runner::{Cmd, CommandRunner};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3600);
const POLL: Duration = Duration::from_secs(2);
const REMIND_EVERY: Duration = Duration::from_secs(300);
const GIB: u64 = 1 << 30;

type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn real_env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn is_off(v: &str) -> bool {
    matches!(v.trim(), "off" | "0" | "false" | "no")
}

// ---------------------------------------------------------------------------
// Queue and lock

/// Held for the length of a heavy build; dropping it releases the lock.
#[must_use = "the lock is released when this is dropped"]
pub struct BuildLock {
    _file: Option<File>,
}

impl BuildLock {
    fn none() -> Self {
        BuildLock { _file: None }
    }
}

/// A place in line: removed when dropped (and skipped once its process is
/// gone, if it never got the chance).
struct Ticket(PathBuf);

impl Drop for Ticket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Where the lock lives, or `None` when this process shouldn't take it.
fn lock_path(env: Env) -> Option<PathBuf> {
    if env("FORGE_BUILD_LOCK_HELD").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    match env("FORGE_BUILD_LOCK").filter(|v| !v.is_empty()) {
        Some(v) if is_off(&v) => None,
        Some(v) => Some(PathBuf::from(v)),
        None => Some(forge_cache(env)?.join("build.lock")),
    }
}

fn forge_cache(env: Env) -> Option<PathBuf> {
    let cache = env("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| Path::new(&h).join(".cache")))?;
    Some(cache.join("forge"))
}

/// The ticket folder that goes with a lock file.
fn queue_dir(lock: &Path) -> PathBuf {
    lock.with_file_name(format!(
        "{}-queue",
        lock.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "build".into())
    ))
}

fn timeout(env: Env) -> Duration {
    env("FORGE_BUILD_LOCK_TIMEOUT")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TIMEOUT)
}

/// Takes a place in the machine's build queue for `what` (e.g. "forge run
/// ios"), waits for its turn, and lowers this process's priority for the
/// build. Unit tests never touch the real queue.
pub fn acquire(what: &str, project_dir: &Path) -> Result<BuildLock, String> {
    acquire_noted(what, project_dir, None)
}

/// [`acquire`] with a note for waiters other than `FORGE_BUILD_LOCK_NOTE`.
pub fn acquire_noted(
    what: &str,
    project_dir: &Path,
    note: Option<&str>,
) -> Result<BuildLock, String> {
    if cfg!(test) {
        return Ok(BuildLock::none());
    }
    lower_priority(&real_env);
    let Some(path) = lock_path(&real_env) else {
        return Ok(BuildLock::none());
    };
    let note = note
        .map(str::to_string)
        .or_else(|| real_env("FORGE_BUILD_LOCK_NOTE"));
    let holder = holder_line(
        std::process::id(),
        &crate::report::timestamp(),
        what,
        project_dir,
        git_branch(project_dir).as_deref(),
        note.as_deref(),
    );
    acquire_at(
        &path,
        std::process::id(),
        &holder,
        timeout(&real_env),
        &mut |line| eprintln!("[forge] {line}"),
        &pid_alive,
    )
}

fn git_branch(dir: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty())
}

fn holder_line(
    pid: u32,
    since: &str,
    what: &str,
    dir: &Path,
    branch: Option<&str>,
    note: Option<&str>,
) -> String {
    let mut line = format!("pid {pid} since {since}: {what} in {}", dir.display());
    if let Some(branch) = branch {
        line.push_str(&format!(" (branch {branch})"));
    }
    if let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) {
        line.push_str(&format!(" [{note}]"));
    }
    line
}

fn read_holder(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown (the lock file names no one yet)".to_string())
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(p) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the process exists.
        let r = unsafe { libc::kill(p, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// The live tickets, oldest first, as (path, pid, line). Tickets whose
/// process is gone are removed.
fn live_tickets(dir: &Path, alive: &dyn Fn(u32) -> bool) -> Vec<(PathBuf, u32, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tickets: Vec<(String, PathBuf, u32)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let pid = name.rsplit('-').next()?.parse::<u32>().ok()?;
            Some((name, e.path(), pid))
        })
        .collect();
    tickets.sort();
    tickets
        .into_iter()
        .filter_map(|(_, path, pid)| {
            if alive(pid) {
                let line = std::fs::read_to_string(&path).unwrap_or_default();
                Some((path, pid, line.trim().to_string()))
            } else {
                let _ = std::fs::remove_file(&path);
                None
            }
        })
        .collect()
}

fn take_ticket(dir: &Path, pid: u32, holder: &str) -> Result<Ticket, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{nanos:024}-{pid}"));
    std::fs::write(&path, format!("{holder}\n"))
        .map_err(|e| format!("cannot join the build queue at {}: {e}", path.display()))?;
    Ok(Ticket(path))
}

/// Joins the queue, waits until this ticket is first and the lock is free,
/// then takes the lock and writes `holder` into it.
fn acquire_at(
    path: &Path,
    pid: u32,
    holder: &str,
    limit: Duration,
    say: &mut dyn FnMut(&str),
    alive: &dyn Fn(u32) -> bool,
) -> Result<BuildLock, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    // Never truncate on open: the line belongs to whoever holds the lock.
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("cannot open the build lock {}: {e}", path.display()))?;
    let queue = queue_dir(path);
    let ticket = take_ticket(&queue, pid, holder)?;
    let started = Instant::now();
    let mut next_reminder = REMIND_EVERY;
    let mut last_place = 0;
    loop {
        let tickets = live_tickets(&queue, alive);
        // Our own ticket is always live; a missing one (deleted by hand)
        // counts as first rather than waiting forever.
        let place = tickets
            .iter()
            .position(|(p, _, _)| *p == ticket.0)
            .unwrap_or(0);
        if place == 0 {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) => {
                    return Err(format!("cannot lock {}: {e}", path.display()));
                }
            }
        }
        let waited = started.elapsed();
        let place = place + 1;
        if last_place == 0 || place < last_place || waited >= next_reminder {
            let message = if last_place == 0 {
                format!(
                    "Another build is running on this machine, so this one is queued: number {place} in line (`forge queue status` shows the queue; waits up to {} min, FORGE_BUILD_LOCK_TIMEOUT). Building now: {}",
                    limit.as_secs().div_ceil(60),
                    read_holder(path)
                )
            } else {
                format!(
                    "Still queued after {} min: number {place} in line. Building now: {}",
                    waited.as_secs() / 60,
                    read_holder(path)
                )
            };
            say(&message);
            if waited >= next_reminder {
                next_reminder += REMIND_EVERY;
            }
            last_place = place;
        }
        if waited >= limit {
            return Err(format!(
                "gave up after {} min in the build queue. Building now: {}. \
                 If that build is stuck, stop its process (its place frees itself when it exits); \
                 set FORGE_BUILD_LOCK_TIMEOUT to wait longer.",
                limit.as_secs().div_ceil(60),
                read_holder(path)
            ));
        }
        std::thread::sleep(
            POLL.min(limit.saturating_sub(waited))
                .max(Duration::from_millis(10)),
        );
    }
    drop(ticket);
    if last_place != 0 {
        say(&format!(
            "Our turn after {} s in the queue; building now.",
            started.elapsed().as_secs()
        ));
    }
    // Best effort: a lock nobody can describe still works.
    let _ = file
        .set_len(0)
        .and_then(|_| file.rewind())
        .and_then(|_| writeln!(file, "{holder}"))
        .and_then(|_| file.flush());
    Ok(BuildLock { _file: Some(file) })
}

/// What `forge queue status` shows.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct QueueStatus {
    pub lock: Option<PathBuf>,
    /// The build holding the lock now, if any.
    pub building: Option<String>,
    /// Waiting builds, first in line first.
    pub waiting: Vec<String>,
}

fn status_at(path: &Path, alive: &dyn Fn(u32) -> bool) -> QueueStatus {
    let building = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => match file.try_lock() {
            Ok(()) => None,
            Err(_) => Some(read_holder(path)),
        },
        Err(_) => None,
    };
    let waiting = live_tickets(&queue_dir(path), alive)
        .into_iter()
        .map(|(_, _, line)| line)
        .collect();
    QueueStatus {
        lock: Some(path.to_path_buf()),
        building,
        waiting,
    }
}

/// `forge queue status`.
pub fn queue_status() -> String {
    let Some(path) = lock_path(&|k| match k {
        // Status is about the machine, even from inside a queued build.
        "FORGE_BUILD_LOCK_HELD" => None,
        _ => real_env(k),
    }) else {
        crate::report::set_data(serde_json::json!({"enabled": false}));
        return "The build queue is off here (FORGE_BUILD_LOCK=off).\n".into();
    };
    let status = status_at(&path, &pid_alive);
    crate::report::set_data(serde_json::to_value(&status).unwrap_or_default());
    let mut out = String::new();
    match &status.building {
        Some(b) => out.push_str(&format!("Building now: {b}\n")),
        None => out.push_str("Nothing is building.\n"),
    }
    if status.waiting.is_empty() {
        out.push_str("Nobody is waiting.\n");
    } else {
        out.push_str(&format!("Waiting ({}):\n", status.waiting.len()));
        for (i, w) in status.waiting.iter().enumerate() {
            out.push_str(&format!("  {}. {w}\n", i + 1));
        }
    }
    out
}

/// `forge queue run -- <command>`: runs any command as a queued, capped
/// build and returns its exit code. Its children see
/// `FORGE_BUILD_LOCK_HELD`, so a `forge build ios` inside doesn't queue
/// behind it.
pub fn queue_run(command: &[String], note: Option<&str>) -> Result<i32, String> {
    let (program, args) = command
        .split_first()
        .ok_or("nothing to run: `forge queue run -- <command> [args...]`")?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let what = format!("forge queue run -- {}", command.join(" "));
    let _lock = acquire_noted(&what, &cwd, note)?;
    let cmd = confine(
        Cmd::new(program)
            .args(args)
            .env("FORGE_BUILD_LOCK_HELD", std::process::id().to_string()),
    )?;
    let mut child = std::process::Command::new(&cmd.program);
    child.args(&cmd.args);
    for (k, v) in &cmd.env {
        child.env(k, v);
    }
    let status = child
        .status()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    if let Some(code) = status.code() {
        return Ok(code);
    }
    Err(format!(
        "`{}` {status}{}",
        command.join(" "),
        killed_hint(&status.to_string())
    ))
}

// ---------------------------------------------------------------------------
// Caps

/// The caps for one heavy command.
#[derive(Debug, Clone, PartialEq)]
struct Limits {
    memory_max: Option<u64>,
    cpu_quota_percent: u32,
    jobs: u32,
}

/// What a machine can take: memory for the build with room left for
/// everything else (at least 2 GiB, or a quarter of the machine), every core
/// but one, and a Cargo job per 2 GiB the build may use.
fn default_limits(mem_total: u64, cores: u32) -> Limits {
    let keep = (mem_total / 4).max(2 * GIB);
    let memory = mem_total.saturating_sub(keep).max(GIB);
    let spare = cores.saturating_sub(1).max(1);
    Limits {
        memory_max: Some(memory),
        cpu_quota_percent: spare * 100,
        jobs: spare.min(((memory / (2 * GIB)) as u32).max(1)),
    }
}

/// `~/.config/forge/build-host.toml`: marks a shared build host, which
/// never builds uncapped, and can set the caps.
#[derive(Debug, Default, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct HostConfig {
    memory_max_gib: Option<f64>,
    cpu_quota_percent: Option<u32>,
    jobs: Option<u32>,
}

fn host_config_path(env: Env) -> Option<PathBuf> {
    let base = env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("forge").join("build-host.toml"))
}

fn read_host_config(env: Env) -> Result<Option<HostConfig>, String> {
    let Some(path) = host_config_path(env) else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(_) => Ok(None),
    }
}

fn limits(env: Env, host: Option<&HostConfig>, mem_total: u64, cores: u32) -> Limits {
    let mut l = default_limits(mem_total, cores);
    if let Some(h) = host {
        if let Some(gib) = h.memory_max_gib {
            l.memory_max = Some((gib * GIB as f64) as u64);
        }
        if let Some(q) = h.cpu_quota_percent {
            l.cpu_quota_percent = q.max(100);
        }
        if let Some(j) = h.jobs {
            l.jobs = j.max(1);
        }
    }
    if let Some(v) = env("FORGE_BUILD_MEMORY_MAX") {
        if is_off(&v) {
            l.memory_max = None;
        } else if let Ok(gib) = v.trim().parse::<f64>() {
            l.memory_max = Some((gib * GIB as f64) as u64);
        }
    }
    if let Some(j) = env("FORGE_BUILD_JOBS").and_then(|v| v.trim().parse::<u32>().ok()) {
        l.jobs = j.max(1);
    }
    l
}

fn mem_total() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("MemTotal:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(8 * GIB)
}

fn cores() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2)
}

/// `systemd-run --user` needs the user's runtime directory, which a
/// non-login session (Remote Control, cron) may not have in its
/// environment.
fn runtime_dir(env: Env) -> Option<String> {
    env("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .or_else(|| {
            #[cfg(unix)]
            {
                // SAFETY: getuid cannot fail.
                let dir = format!("/run/user/{}", unsafe { libc::getuid() });
                Path::new(&dir).is_dir().then_some(dir)
            }
            #[cfg(not(unix))]
            None
        })
}

/// Whether a capped scope works here, or why not. Checked once per process.
fn scope_support() -> &'static Result<(), String> {
    static SUPPORT: OnceLock<Result<(), String>> = OnceLock::new();
    SUPPORT.get_or_init(|| {
        if !cfg!(target_os = "linux") {
            return Err("not Linux".into());
        }
        let mut probe = std::process::Command::new("systemd-run");
        probe
            .args(["--user", "--scope", "--quiet", "--collect", "-p", "MemoryMax=1G", "--", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        if let Some(dir) = runtime_dir(&real_env) {
            probe.env("XDG_RUNTIME_DIR", dir);
        }
        match probe.output() {
            Err(e) => return Err(format!("systemd-run is not available ({e})")),
            Ok(o) if !o.status.success() => {
                return Err(format!(
                    "`systemd-run --user --scope` failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                ))
            }
            Ok(_) => {}
        }
        // A scope only enforces MemoryMax if the user's systemd was given
        // the memory controller.
        #[cfg(unix)]
        {
            // SAFETY: getuid cannot fail.
            let uid = unsafe { libc::getuid() };
            let controllers = format!(
                "/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/cgroup.controllers"
            );
            match std::fs::read_to_string(&controllers) {
                Ok(c) if c.split_whitespace().any(|w| w == "memory") => {}
                Ok(_) => {
                    return Err(
                        "the memory controller isn't delegated to the user's systemd, so MemoryMax would do nothing"
                            .into(),
                    )
                }
                Err(e) => return Err(format!("cannot read {controllers} ({e})")),
            }
        }
        Ok(())
    })
}

/// The command as a capped heavy build: in a systemd scope with the
/// machine's memory and CPU caps, and with fewer Cargo jobs. Unchanged in
/// unit tests, inside an already capped build, or with
/// `FORGE_BUILD_MEMORY_MAX=off`. Fails on a shared build host where the
/// caps can't be applied.
pub fn confine(cmd: Cmd) -> Result<Cmd, String> {
    if cfg!(test) {
        return Ok(cmd);
    }
    let host = read_host_config(&real_env)?;
    let l = limits(&real_env, host.as_ref(), mem_total(), cores());
    let mut cmd = cmd;
    if real_env("CARGO_BUILD_JOBS").is_none()
        && !cmd.env.iter().any(|(k, _)| k == "CARGO_BUILD_JOBS")
    {
        cmd = cmd.env("CARGO_BUILD_JOBS", l.jobs.to_string());
    }
    if real_env("FORGE_BUILD_SCOPE").is_some() {
        return Ok(cmd); // already inside a capped scope
    }
    let Some(memory) = l.memory_max else {
        return Ok(cmd);
    };
    match scope_support() {
        Ok(()) => Ok(scoped(cmd, &l, memory, runtime_dir(&real_env))),
        Err(why) if host.is_some() => Err(format!(
            "this machine is a shared build host ({}), and builds there only run with a memory cap, but it can't be applied: {why}. Run scripts/setup-build-host.sh (docs/builds.md#the-build-queue)",
            host_config_path(&real_env).map(|p| p.display().to_string()).unwrap_or_default()
        )),
        Err(why) => {
            static WARNED: OnceLock<()> = OnceLock::new();
            WARNED.get_or_init(|| {
                eprintln!("[forge] note: building without a memory cap ({why}); see docs/builds.md#the-build-queue");
            });
            Ok(cmd)
        }
    }
}

fn scoped(cmd: Cmd, l: &Limits, memory: u64, runtime_dir: Option<String>) -> Cmd {
    let mut wrapped = Cmd::new("systemd-run").args(["--user", "--scope", "--quiet", "--collect"]);
    // Throttle first, then kill: no swap, so a runaway build ends quickly
    // instead of thrashing the disk.
    for p in [
        format!("MemoryHigh={}", memory / 10 * 9),
        format!("MemoryMax={memory}"),
        "MemorySwapMax=0".to_string(),
        format!("CPUQuota={}%", l.cpu_quota_percent),
        "CPUWeight=20".to_string(),
        "IOWeight=20".to_string(),
    ] {
        wrapped = wrapped.arg("-p").arg(p);
    }
    wrapped = wrapped.arg("--").arg(&cmd.program).args(&cmd.args);
    wrapped.cwd = cmd.cwd;
    wrapped.env = cmd.env;
    if let Some(dir) = runtime_dir {
        wrapped = wrapped.env("XDG_RUNTIME_DIR", dir);
    }
    wrapped.env("FORGE_BUILD_SCOPE", "1")
}

fn killed_hint(status: &str) -> &'static str {
    if status.contains("signal: 9") || status.contains("SIGKILL") {
        " (killed: it may have hit the build memory cap, which stops a build before it can freeze the machine; see docs/builds.md#the-build-queue)"
    } else {
        ""
    }
}

/// Runs one heavy step (Cargo for iOS, `xtool dev build`) capped.
pub fn run_heavy(runner: &dyn CommandRunner, cmd: Cmd) -> Result<(), String> {
    crate::commands::run_explained(runner, confine(cmd)?).map_err(|e| {
        let hint = killed_hint(&e);
        format!("{e}{hint}")
    })
}

/// nice 10 and best-effort I/O priority 7 for this process and everything
/// it starts, so the machine's other users come first. Never raises it.
fn lower_priority(env: Env) {
    if env("FORGE_BUILD_NICE").is_some_and(|v| is_off(&v)) {
        return;
    }
    #[cfg(unix)]
    // SAFETY: plain syscalls on this process; failures are ignored.
    unsafe {
        let current = libc::getpriority(libc::PRIO_PROCESS, 0);
        if current < 10 {
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
        }
    }
    #[cfg(target_os = "linux")]
    // SAFETY: ioprio_set(IOPRIO_WHO_PROCESS, self, best-effort class 2,
    // level 7); failure leaves the I/O priority unchanged.
    unsafe {
        const IOPRIO_WHO_PROCESS: libc::c_long = 1;
        const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
        const IOPRIO_CLASS_BE: libc::c_long = 2;
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0 as libc::c_long,
            (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 7,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    fn all_alive(_: u32) -> bool {
        true
    }

    #[test]
    fn lock_lives_in_the_forge_cache_unless_overridden() {
        assert_eq!(
            lock_path(&env_of(&[("HOME", "/home/red")])),
            Some(PathBuf::from("/home/red/.cache/forge/build.lock"))
        );
        assert_eq!(
            lock_path(&env_of(&[("HOME", "/home/red"), ("XDG_CACHE_HOME", "/c")])),
            Some(PathBuf::from("/c/forge/build.lock"))
        );
        assert_eq!(
            lock_path(&env_of(&[("FORGE_BUILD_LOCK", "/tmp/x.lock")])),
            Some(PathBuf::from("/tmp/x.lock"))
        );
        assert_eq!(lock_path(&env_of(&[("FORGE_BUILD_LOCK", "off")])), None);
        // A queued build's children don't queue behind it.
        assert_eq!(
            lock_path(&env_of(&[("HOME", "/h"), ("FORGE_BUILD_LOCK_HELD", "123")])),
            None
        );
        assert_eq!(
            queue_dir(Path::new("/c/forge/build.lock")),
            PathBuf::from("/c/forge/build-queue")
        );
    }

    #[test]
    fn timeout_defaults_to_an_hour() {
        assert_eq!(timeout(&env_of(&[])), DEFAULT_TIMEOUT);
        assert_eq!(
            timeout(&env_of(&[("FORGE_BUILD_LOCK_TIMEOUT", "90")])),
            Duration::from_secs(90)
        );
    }

    #[test]
    fn holder_line_names_the_process_build_branch_and_note() {
        let line = holder_line(
            42,
            "2026-11-14T15-10-00",
            "forge run ios",
            Path::new("/home/red/ixOS/examples/device-lab"),
            Some("claude/ota-fix"),
            Some("PR #189"),
        );
        assert_eq!(
            line,
            "pid 42 since 2026-11-14T15-10-00: forge run ios in /home/red/ixOS/examples/device-lab (branch claude/ota-fix) [PR #189]"
        );
    }

    #[test]
    fn second_caller_waits_names_the_holder_then_gives_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("build.lock");
        let first = acquire_at(
            &path,
            1,
            "pid 1 first",
            DEFAULT_TIMEOUT,
            &mut |_| {},
            &all_alive,
        )
        .unwrap();
        let mut said = Vec::new();
        let err = acquire_at(
            &path,
            2,
            "pid 2",
            Duration::from_millis(50),
            &mut |l| said.push(l.to_string()),
            &all_alive,
        )
        .err()
        .expect("the lock is held");
        assert!(said[0].contains("number 1 in line"), "{said:?}");
        assert!(said[0].contains("pid 1 first"), "{said:?}");
        assert!(err.contains("pid 1 first"), "{err}");
        // The waiter that gave up left the queue.
        assert!(live_tickets(&queue_dir(&path), &all_alive).is_empty());
        drop(first);
        let _second =
            acquire_at(&path, 3, "pid 3", DEFAULT_TIMEOUT, &mut |_| {}, &all_alive).unwrap();
        assert_eq!(read_holder(&path), "pid 3");
    }

    #[test]
    fn builds_go_in_the_order_they_arrived() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("build.lock");
        // pid 10 builds; pid 11 queued earlier than us.
        let first = acquire_at(
            &path,
            10,
            "pid 10",
            DEFAULT_TIMEOUT,
            &mut |_| {},
            &all_alive,
        )
        .unwrap();
        let earlier = take_ticket(&queue_dir(&path), 11, "pid 11 earlier").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        drop(first);
        // The lock is free, but pid 11 is ahead of us.
        let mut said = Vec::new();
        let err = acquire_at(
            &path,
            12,
            "pid 12",
            Duration::from_millis(50),
            &mut |l| said.push(l.to_string()),
            &all_alive,
        );
        assert!(err.is_err());
        assert!(said[0].contains("number 2 in line"), "{said:?}");
        let status = status_at(&path, &all_alive);
        assert_eq!(status.building, None);
        assert_eq!(status.waiting, vec!["pid 11 earlier".to_string()]);
        // A ticket whose process is gone doesn't hold anyone up.
        let dead_11 = |pid: u32| pid != 11;
        let _lock = acquire_at(
            &path,
            12,
            "pid 12",
            Duration::from_secs(5),
            &mut |_| {},
            &dead_11,
        )
        .unwrap();
        assert!(!earlier.0.exists());
        let status = status_at(&path, &all_alive);
        assert_eq!(status.building.as_deref(), Some("pid 12"));
        assert!(status.waiting.is_empty());
    }

    #[test]
    fn waiter_gets_the_lock_when_the_holder_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("build.lock");
        let first =
            acquire_at(&path, 1, "pid 1", DEFAULT_TIMEOUT, &mut |_| {}, &all_alive).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(first);
        });
        let mut said = Vec::new();
        let _lock = acquire_at(
            &path,
            2,
            "pid 2",
            Duration::from_secs(30),
            &mut |l| said.push(l.to_string()),
            &all_alive,
        )
        .unwrap();
        releaser.join().unwrap();
        assert!(said.last().unwrap().contains("building now"), "{said:?}");
    }

    #[test]
    fn caps_leave_room_for_the_rest_of_the_machine() {
        // An 8 GiB, 4-core laptop: 6 GiB, 3 cores, 3 jobs.
        assert_eq!(
            default_limits(8 * GIB, 4),
            Limits {
                memory_max: Some(6 * GIB),
                cpu_quota_percent: 300,
                jobs: 3
            }
        );
        // 16 GiB, 8 cores: 12 GiB, 7 cores, 6 jobs (2 GiB each).
        assert_eq!(
            default_limits(16 * GIB, 8),
            Limits {
                memory_max: Some(12 * GIB),
                cpu_quota_percent: 700,
                jobs: 6
            }
        );
        // Tiny machines still build, one job at a time.
        let tiny = default_limits(2 * GIB, 1);
        assert_eq!(
            (tiny.memory_max, tiny.cpu_quota_percent, tiny.jobs),
            (Some(GIB), 100, 1)
        );
    }

    #[test]
    fn host_file_and_environment_override_the_caps() {
        let host: HostConfig =
            toml::from_str("memory_max_gib = 5\ncpu_quota_percent = 200\njobs = 2\n").unwrap();
        let l = limits(&env_of(&[]), Some(&host), 16 * GIB, 8);
        assert_eq!(
            l,
            Limits {
                memory_max: Some(5 * GIB),
                cpu_quota_percent: 200,
                jobs: 2
            }
        );
        let l = limits(
            &env_of(&[("FORGE_BUILD_MEMORY_MAX", "off"), ("FORGE_BUILD_JOBS", "1")]),
            Some(&host),
            16 * GIB,
            8,
        );
        assert_eq!((l.memory_max, l.jobs), (None, 1));
        // An empty file just marks the machine as a shared build host.
        assert_eq!(
            toml::from_str::<HostConfig>("").unwrap(),
            HostConfig::default()
        );
        assert!(toml::from_str::<HostConfig>("memory = 3").is_err());
    }

    #[test]
    fn scoped_command_keeps_its_directory_and_environment() {
        let cmd = Cmd::new("xtool")
            .args(["dev", "build"])
            .cwd("/app")
            .env("CARGO_BUILD_JOBS", "3");
        let l = default_limits(8 * GIB, 4);
        let s = scoped(cmd, &l, 6 * GIB, Some("/run/user/1000".into()));
        assert_eq!(s.program, "systemd-run");
        let line = s.display();
        assert!(line.contains("--user --scope"), "{line}");
        assert!(line.contains(&format!("MemoryMax={}", 6 * GIB)), "{line}");
        assert!(line.contains("MemorySwapMax=0"), "{line}");
        assert!(line.contains("CPUQuota=300%"), "{line}");
        assert!(line.ends_with("-- xtool dev build"), "{line}");
        assert_eq!(s.cwd, Some(PathBuf::from("/app")));
        let env: HashMap<_, _> = s.env.iter().cloned().collect();
        assert_eq!(env["CARGO_BUILD_JOBS"], "3");
        assert_eq!(env["XDG_RUNTIME_DIR"], "/run/user/1000");
        assert_eq!(env["FORGE_BUILD_SCOPE"], "1");
    }

    #[test]
    fn a_killed_build_points_at_the_memory_cap() {
        assert!(
            killed_hint("`xtool dev build` failed (signal: 9 (SIGKILL))").contains("memory cap")
        );
        assert_eq!(killed_hint("exit status: 1"), "");
    }
}
