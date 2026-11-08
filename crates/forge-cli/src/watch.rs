//! Watching the app during a device run (`forge ui run`, `forge ui explore`):
//! its own log lines while the run lasts, and the crash reports it left
//! behind (docs/forge-m6.md, M6.1). Both go into the run's evidence bundle,
//! so a failure comes with what the app said before it and, after a crash,
//! the symbolicated place it crashed.
//!
//! Best effort: a phone that won't stream its log or list its reports
//! doesn't fail the run; the bundle says what couldn't be collected.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forge_device::{app_crash_reports, pick_device, DeviceBackend, LogFilter};
use serde_json::{json, Value};

/// Opens a connection to the phones. Called once on the watching thread
/// and once on the caller's, so neither shares a connection.
pub type Connect = fn() -> Result<Box<dyn DeviceBackend>, String>;

/// usbmuxd, as `forge device` uses it.
pub fn usb() -> Result<Box<dyn DeviceBackend>, String> {
    forge_device::UsbBackend::new()
        .map(|b| Box::new(b) as Box<dyn DeviceBackend>)
        .map_err(|e| e.to_string())
}

/// Lines kept from one run; the oldest go first.
const MAX_LINES: usize = 5000;
/// Lines of the log end attached to a failure.
pub const TAIL: usize = 20;
/// How long `finish` waits for the log thread to notice it should stop.
const STOP_WAIT: Duration = Duration::from_secs(2);

pub struct Watch {
    connect: Connect,
    udid: String,
    /// The app's executable name (forge.toml `[ios] xtool_product`).
    process: String,
    project: PathBuf,
    started: Instant,
    lines: Arc<Mutex<VecDeque<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
    /// Crash reports for the app already on the phone before the run.
    before: Result<Vec<String>, String>,
}

impl Watch {
    /// Lists the app's crash reports, then starts reading its log lines.
    pub fn start(connect: Connect, udid: &str, process: &str, project: &Path) -> Watch {
        let before = connect().and_then(|b| crash_names(b.as_ref(), udid, process));
        let lines = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let thread = {
            let (lines, stop) = (lines.clone(), stop.clone());
            let udid = udid.to_owned();
            let filter = LogFilter {
                process: Some(process.to_owned()),
                contains: None,
            };
            std::thread::Builder::new()
                .name("forge-watch-log".into())
                .spawn(move || {
                    let backend = connect()?;
                    let device =
                        pick_device(backend.as_ref(), Some(&udid)).map_err(|e| e.to_string())?;
                    backend
                        .syslog(&device, &mut |line| {
                            if stop.load(Ordering::Relaxed) {
                                return false;
                            }
                            if filter.matches(line) {
                                let ms = started.elapsed().as_millis();
                                let mut lines = lines.lock().unwrap_or_else(|p| p.into_inner());
                                if lines.len() == MAX_LINES {
                                    lines.pop_front();
                                }
                                lines.push_back(format!("+{ms}ms {line}"));
                            }
                            true
                        })
                        .map_err(|e| e.to_string())
                })
                .ok()
        };
        Watch {
            connect,
            udid: udid.to_owned(),
            process: process.to_owned(),
            project: project.to_path_buf(),
            started,
            lines,
            stop,
            thread,
            before,
        }
    }

    /// Stops reading the log, writes `device.log` into `dir` and, when
    /// `crashes` is set (the run failed or the app closed), copies the
    /// app's new crash reports into `dir/crashes/` and symbolicates them
    /// against the app's build. Returns the `watch` object of the bundle.
    pub fn finish(mut self, dir: &Path, crashes: bool) -> Finished {
        self.stop.store(true, Ordering::Relaxed);
        let mut notes = Vec::new();
        if let Some(thread) = self.thread.take() {
            // The thread notices on the next line; a silent phone may never
            // send one, so don't wait for it longer than STOP_WAIT.
            let deadline = Instant::now() + STOP_WAIT;
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if thread.is_finished() {
                match thread.join() {
                    Ok(Err(e)) => notes.push(format!("app log not read: {e}")),
                    Err(_) => notes.push("app log not read: the log reader stopped".into()),
                    Ok(Ok(())) => {}
                }
            }
        } else {
            notes.push("app log not read: could not start the log reader".into());
        }
        let lines: Vec<String> = self
            .lines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect();
        let mut artifacts = Vec::new();
        let _ = std::fs::create_dir_all(dir);
        if std::fs::write(dir.join("device.log"), lines.join("\n") + "\n").is_ok() {
            artifacts.push(("device_log", "device.log".to_string()));
        }
        let tail: Vec<String> = lines.iter().rev().take(TAIL).rev().cloned().collect();
        let mut crash_list = Vec::new();
        if crashes {
            match self.new_crashes(dir) {
                Ok(found) => {
                    for (file, entry) in found {
                        if let Ok(rel) = file.strip_prefix(dir) {
                            artifacts.push(("crash", rel.display().to_string()));
                        }
                        crash_list.push(entry);
                    }
                }
                Err(e) => notes.push(format!("crash reports not checked: {e}")),
            }
        }
        let value = json!({
            "process": self.process,
            "seconds": self.started.elapsed().as_secs_f64(),
            "log": "device.log",
            "log_lines": lines.len(),
            "log_tail": tail,
            "crashes_checked": crashes,
            "crashes": crash_list,
            "notes": notes,
        });
        Finished {
            value,
            tail,
            artifacts,
        }
    }

    fn new_crashes(&self, dir: &Path) -> Result<Vec<(PathBuf, Value)>, String> {
        let before = self.before.clone()?;
        let backend = (self.connect)()?;
        let device = pick_device(backend.as_ref(), Some(&self.udid)).map_err(|e| e.to_string())?;
        let now = crash_names(backend.as_ref(), &self.udid, &self.process)?;
        let fresh: Vec<&String> = now.iter().filter(|n| !before.contains(n)).collect();
        let dest = dir.join("crashes");
        let mut files = Vec::new();
        for name in fresh {
            let bytes = backend
                .pull_crash_report(&device, name)
                .map_err(|e| format!("{name}: {e}"))?;
            std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            let file = dest.join(name.rsplit('/').next().unwrap_or(name));
            std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            files.push(file);
        }
        let options = crate::crash::options(Some(&self.project), &[], false);
        Ok(crate::crash::analyze_pulled(&files, &options)
            .into_iter()
            .map(|(file, analysis)| {
                let entry = crate::crash::entry(&file, &analysis);
                (file, entry)
            })
            .collect())
    }
}

/// What a watch collected.
pub struct Finished {
    /// The bundle's `watch` object.
    pub value: Value,
    /// The last log lines, for the failed step.
    pub tail: Vec<String>,
    /// Files written into the run folder: (kind, path relative to it).
    pub artifacts: Vec<(&'static str, String)>,
}

fn crash_names(
    backend: &dyn DeviceBackend,
    udid: &str,
    process: &str,
) -> Result<Vec<String>, String> {
    let device = pick_device(backend, Some(udid)).map_err(|e| e.to_string())?;
    let all = backend.crash_reports(&device).map_err(|e| e.to_string())?;
    Ok(app_crash_reports(&all, process))
}

/// The app's executable name from its forge.toml, when it has one.
pub fn product(app_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app_dir.join("forge.toml")).ok()?;
    let config = crate::config::ForgeConfig::parse(&text).ok()?;
    Some(crate::app_config::xtool_product(&config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_device::{Connection, DeviceInfo, DeviceRef, Result};
    use std::sync::atomic::AtomicUsize;

    /// How many times the fake listed crash reports: the first listing is
    /// "before the run", later ones "after".
    static LISTINGS: AtomicUsize = AtomicUsize::new(0);

    struct Fake;

    impl DeviceBackend for Fake {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Ok(vec![DeviceRef {
                udid: "0000".into(),
                connection: Connection::Usb,
            }])
        }
        fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
            Ok(DeviceInfo {
                udid: device.udid.clone(),
                connection: Connection::Usb,
                name: "Test".into(),
                product_type: "iPhone16,2".into(),
                ios_version: "26.3.1".into(),
                build_version: "x".into(),
                developer_mode: Some(true),
            })
        }
        fn syslog(&self, _: &DeviceRef, on_line: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            for line in [
                "Nov  8 13:00:01 iPhone GoodDogXtool[9] <Notice>: [today] plan ready",
                "Nov  8 13:00:01 iPhone SpringBoard[2] <Notice>: icon moved",
                "Nov  8 13:00:02 iPhone GoodDogXtool[9] <Error>: [exercise] outcome rejected",
            ] {
                if !on_line(line) {
                    return Ok(());
                }
            }
            // Then a phone that keeps logging, until told to stop.
            while on_line("Nov  8 13:00:03 iPhone backboardd[3] <Notice>: tick") {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            let mut names = vec!["GoodDogXtool-2026-11-07-090000.ips".to_string()];
            if LISTINGS.fetch_add(1, Ordering::SeqCst) > 0 {
                names.push("GoodDogXtool-2026-11-08-130002.ips".into());
                names.push("Mail-2026-11-08-130002.ips".into());
            }
            Ok(names)
        }
        fn pull_crash_report(&self, _: &DeviceRef, name: &str) -> Result<Vec<u8>> {
            Ok(format!("not a real report: {name}").into_bytes())
        }
    }

    fn fake() -> std::result::Result<Box<dyn DeviceBackend>, String> {
        Ok(Box::new(Fake))
    }

    #[test]
    fn keeps_the_apps_log_lines_and_pulls_only_its_new_crashes() {
        let tmp = tempfile::tempdir().unwrap();
        let watch = Watch::start(fake, "0000", "GoodDogXtool", tmp.path());
        std::thread::sleep(Duration::from_millis(100));
        let done = watch.finish(tmp.path(), true);
        let v = &done.value;
        assert_eq!(v["log_lines"], 2, "{v}");
        assert!(
            done.tail[1].contains("[exercise] outcome rejected"),
            "{:?}",
            done.tail
        );
        assert!(done.tail[0].starts_with('+'));
        let log = std::fs::read_to_string(tmp.path().join("device.log")).unwrap();
        assert!(!log.contains("SpringBoard") && !log.contains("backboardd"));
        // Only the app's report written during the run, not the old one or
        // another app's.
        let crashes = v["crashes"].as_array().unwrap();
        assert_eq!(crashes.len(), 1, "{v}");
        assert!(tmp
            .path()
            .join("crashes/GoodDogXtool-2026-11-08-130002.ips")
            .is_file());
        assert!(!tmp
            .path()
            .join("crashes/GoodDogXtool-2026-11-07-090000.ips")
            .exists());
        let kinds: Vec<&str> = done.artifacts.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, vec!["device_log", "crash"]);
    }
}
