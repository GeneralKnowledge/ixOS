//! One heavy build at a time on a machine.
//!
//! `forge build ios`, `forge run ios` (its build part), `forge publish ios`
//! and `scripts/check-ios-xtool.sh` each take an exclusive lock on one file
//! before compiling, so several sessions sharing a build machine (the device
//! host laptop) queue instead of running Cargo, SwiftPM and xtool at once and
//! starving everything else on it (Remote Control, the over-the-air install
//! server). A second caller prints who holds the lock and waits.
//!
//! The lock is `flock(2)` (`File::lock`), the same lock as util-linux
//! `flock`, which the build check script uses. The kernel drops it when the
//! holder exits, however it exits, so a crashed or killed build never leaves
//! a stale lock. What can stay behind is a build that hangs: the wait gives
//! up after `FORGE_BUILD_LOCK_TIMEOUT` seconds and names the holder's pid.
//!
//! The lock file's one line says who holds it. Environment:
//!
//! - `FORGE_BUILD_LOCK`: the lock file (default
//!   `${XDG_CACHE_HOME:-~/.cache}/forge/build.lock`), or `off` for no lock.
//! - `FORGE_BUILD_LOCK_TIMEOUT`: seconds to wait (default 3600).
//! - `FORGE_BUILD_LOCK_NOTE`: who is building (a thread, PR or person),
//!   shown to whoever waits.
//! - `FORGE_BUILD_LOCK_HELD`: set by a caller that already holds the lock
//!   (the build check script), so the `forge build ios` it runs doesn't wait
//!   on its own parent.
//! - `FORGE_BUILD_NICE=0`: keep normal CPU and disk priority. By default a
//!   heavy build runs at nice 10 and best-effort I/O priority 7, so an
//!   interactive session or a download on the same machine stays responsive.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3600);
const POLL: Duration = Duration::from_secs(2);
const REMIND_EVERY: Duration = Duration::from_secs(300);

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

/// Where the lock lives, or `None` when this process shouldn't take it.
fn lock_path(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if env("FORGE_BUILD_LOCK_HELD").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    match env("FORGE_BUILD_LOCK").filter(|v| !v.is_empty()) {
        Some(v) if matches!(v.as_str(), "off" | "0" | "false" | "no") => None,
        Some(v) => Some(PathBuf::from(v)),
        None => {
            let cache = env("XDG_CACHE_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| env("HOME").map(|h| Path::new(&h).join(".cache")))?;
            Some(cache.join("forge").join("build.lock"))
        }
    }
}

fn timeout(env: &dyn Fn(&str) -> Option<String>) -> Duration {
    env("FORGE_BUILD_LOCK_TIMEOUT")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TIMEOUT)
}

/// Takes the machine's build lock for `what` (e.g. "forge run ios"),
/// waiting while another build holds it, and lowers this process's
/// priority for the build. Unit tests never touch the real lock.
pub fn acquire(what: &str, project_dir: &Path) -> Result<BuildLock, String> {
    if cfg!(test) {
        return Ok(BuildLock::none());
    }
    let env = |k: &str| std::env::var(k).ok();
    lower_priority(&env);
    let Some(path) = lock_path(&env) else {
        return Ok(BuildLock::none());
    };
    let holder = describe(what, project_dir, &env);
    acquire_at(&path, &holder, timeout(&env), &mut |line| {
        println!("[forge] {line}")
    })
}

/// The line other callers see while this process holds the lock.
fn describe(what: &str, project_dir: &Path, env: &dyn Fn(&str) -> Option<String>) -> String {
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty());
    holder_line(
        std::process::id(),
        &crate::report::timestamp(),
        what,
        project_dir,
        branch.as_deref(),
        env("FORGE_BUILD_LOCK_NOTE").as_deref(),
    )
}

fn holder_line(
    pid: u32,
    since: &str,
    what: &str,
    project_dir: &Path,
    branch: Option<&str>,
    note: Option<&str>,
) -> String {
    let mut line = format!(
        "pid {pid} since {since}: {what} in {}",
        project_dir.display()
    );
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

/// The lock itself: try, and while someone else holds it, say who and poll
/// until it is free or `limit` passes.
fn acquire_at(
    path: &Path,
    holder: &str,
    limit: Duration,
    say: &mut dyn FnMut(&str),
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
    let started = Instant::now();
    let mut next_reminder = REMIND_EVERY;
    let mut waiting = false;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => {
                return Err(format!("cannot lock {}: {e}", path.display()));
            }
        }
        let waited = started.elapsed();
        if !waiting {
            waiting = true;
            say(&format!(
                "Another build is running on this machine, so this one waits its turn (up to {} min, FORGE_BUILD_LOCK_TIMEOUT). Running now: {}",
                limit.as_secs().div_ceil(60),
                read_holder(path)
            ));
        } else if waited >= next_reminder {
            next_reminder += REMIND_EVERY;
            say(&format!(
                "Still waiting after {} min. Running now: {}",
                waited.as_secs() / 60,
                read_holder(path)
            ));
        }
        if waited >= limit {
            return Err(format!(
                "gave up after {} min waiting for another build on this machine to finish. Running now: {}. \
                 If that build is stuck, stop its process (the lock frees itself when it exits); \
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
    if waiting {
        say(&format!(
            "The other build finished after {} s; building now.",
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

/// nice 10 and best-effort I/O priority 7 for this process and everything
/// it starts, so the machine's other users come first. Never raises it.
fn lower_priority(env: &dyn Fn(&str) -> Option<String>) {
    if env("FORGE_BUILD_NICE").is_some_and(|v| matches!(v.trim(), "0" | "off" | "false" | "no")) {
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
        // The build check script already holds it for its `forge build ios`.
        assert_eq!(
            lock_path(&env_of(&[("HOME", "/h"), ("FORGE_BUILD_LOCK_HELD", "123")])),
            None
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
            "2026-11-14T15:10:00Z",
            "forge run ios",
            Path::new("/home/red/ixOS/examples/device-lab"),
            Some("claude/ota-fix"),
            Some("PR #189"),
        );
        assert_eq!(
            line,
            "pid 42 since 2026-11-14T15:10:00Z: forge run ios in /home/red/ixOS/examples/device-lab (branch claude/ota-fix) [PR #189]"
        );
    }

    #[test]
    fn second_caller_waits_names_the_holder_then_gives_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("build.lock");
        let first = acquire_at(&path, "pid 1 first build", DEFAULT_TIMEOUT, &mut |_| {}).unwrap();
        let mut said = Vec::new();
        let err = acquire_at(&path, "pid 2", Duration::from_millis(50), &mut |l| {
            said.push(l.to_string())
        })
        .err()
        .expect("the lock is held");
        assert!(said[0].contains("waits its turn"), "{said:?}");
        assert!(said[0].contains("pid 1 first build"), "{said:?}");
        assert!(err.contains("pid 1 first build"), "{err}");
        drop(first);
        // Released on drop (as on exit): the next caller gets it and the
        // line now names it.
        let _second = acquire_at(&path, "pid 3 second", DEFAULT_TIMEOUT, &mut |_| {}).unwrap();
        assert_eq!(read_holder(&path), "pid 3 second");
    }

    #[test]
    fn waiter_gets_the_lock_when_the_holder_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("build.lock");
        let first = acquire_at(&path, "pid 1", DEFAULT_TIMEOUT, &mut |_| {}).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(first);
        });
        let mut said = Vec::new();
        let _lock = acquire_at(&path, "pid 2", Duration::from_secs(30), &mut |l| {
            said.push(l.to_string())
        })
        .unwrap();
        releaser.join().unwrap();
        assert!(said.last().unwrap().contains("building now"), "{said:?}");
    }
}
