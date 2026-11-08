//! Watching the app during a device run (`forge ui run`, `forge ui explore`):
//! its own log lines while the run lasts, and the crash reports it left
//! behind (docs/forge-m6.md, M6.1). Both go into the run's evidence bundle,
//! so a failure comes with what the app said before it and, after a crash,
//! the symbolicated place it crashed.
//!
//! Best effort: a phone that won't stream its log or list its reports
//! doesn't fail the run. What couldn't be collected is said, never left to
//! look like a quiet app: the `watch` object has a `log` status
//! (`collected`, `empty`, `unavailable`) and a `crash_reports` status
//! (`not_checked`, `none_new`, `found`, `unavailable`), and a run that
//! wasn't watched at all says why ([`not_watched`]).
//!
//! A [`Watch`] stops its log reader when it is finished or dropped, so a run
//! that ends with an error doesn't leave a thread holding the phone's log
//! service; the reader is given [`STOP_WAIT`] to notice, never more.

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
/// How long `finish` waits for the log reader to stop.
pub const STOP_WAIT: Duration = Duration::from_secs(2);

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
                        .syslog_while(&device, &stop, &mut |line| {
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
        let mut notes = Vec::new();
        let reader = self.stop_reader();
        let log_status = match &reader {
            Err(e) => {
                notes.push(format!("app log not read: {e}"));
                "unavailable"
            }
            Ok(()) if self.line_count() == 0 => "empty",
            Ok(()) => "collected",
        };
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
        let crash_status = if !crashes {
            "not_checked"
        } else {
            match self.new_crashes(dir) {
                Ok(found) => {
                    for (file, entry) in found {
                        if let Ok(rel) = file.strip_prefix(dir) {
                            artifacts.push(("crash", rel.display().to_string()));
                        }
                        crash_list.push(entry);
                    }
                    if crash_list.is_empty() {
                        "none_new"
                    } else {
                        "found"
                    }
                }
                Err(e) => {
                    notes.push(format!("crash reports not checked: {e}"));
                    "unavailable"
                }
            }
        };
        let value = json!({
            "status": "watched",
            "process": self.process,
            "seconds": self.started.elapsed().as_secs_f64(),
            "log": "device.log",
            "log_status": log_status,
            "log_lines": lines.len(),
            "log_tail": tail,
            "crash_reports": crash_status,
            "crashes_checked": crash_status == "none_new" || crash_status == "found",
            "crashes": crash_list,
            "notes": notes,
        });
        Finished {
            value,
            tail,
            artifacts,
        }
    }

    fn line_count(&self) -> usize {
        self.lines.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Tells the log reader to stop and waits up to [`STOP_WAIT`] for it.
    /// `Err` says why the log can't be trusted as complete.
    fn stop_reader(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        let Some(thread) = self.thread.take() else {
            return Err("could not start the log reader".into());
        };
        let deadline = Instant::now() + STOP_WAIT;
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if !thread.is_finished() {
            // Dropping the handle detaches it; it ends on the phone's next
            // line. What it read so far is kept.
            return Err(format!(
                "the log reader didn't stop within {} s; the lines read before are kept",
                STOP_WAIT.as_secs()
            ));
        }
        match thread.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the log reader stopped".into()),
        }
    }

    fn new_crashes(&self, dir: &Path) -> Result<Vec<(PathBuf, Value)>, String> {
        let before = self
            .before
            .clone()
            .map_err(|e| format!("listing them before the run failed: {e}"))?;
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
        let analysed = crate::crash::analyze_pulled(&files, &options);
        // A pulled file that isn't `.ips` isn't analysed: still say so.
        let mut entries: Vec<(PathBuf, Value)> = analysed
            .into_iter()
            .map(|(file, analysis)| {
                let mut entry = crate::crash::entry(&file, &analysis);
                entry["symbolicated"] = json!(symbolication(&entry));
                (file, entry)
            })
            .collect();
        for file in files {
            if !entries.iter().any(|(f, _)| *f == file) {
                let entry = json!({
                    "file": file.display().to_string(),
                    "error": "not an .ips report; not analysed",
                    "symbolicated": "unreadable",
                });
                entries.push((file, entry));
            }
        }
        Ok(entries)
    }
}

impl Drop for Watch {
    /// A run that returned early never called `finish`: the reader still
    /// stops, so it doesn't keep the phone's log service busy.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// How far a crash entry could be read: `matched` (the build that crashed
/// is here: file and line are right), `other_build` (frames point at
/// today's code, not the crashed one), `no_local_build`, `unknown`, or
/// `unreadable` (the report couldn't be analysed at all).
pub fn symbolication(entry: &Value) -> &'static str {
    if entry.get("error").is_some() {
        return "unreadable";
    }
    match entry["build"]["status"].as_str() {
        Some("matched") => "matched",
        Some("other_build") => "other_build",
        Some("not_found") => "no_local_build",
        _ => "unknown",
    }
}

/// The `watch` object of a run that wasn't watched, and why. Nothing was
/// collected, so nothing in it can read as "the app logged nothing".
pub fn not_watched(reason: &str) -> Value {
    json!({
        "status": "not_watched",
        "reason": reason,
        "log_status": "unavailable",
        "crash_reports": "not_checked",
        "crashes_checked": false,
        "crashes": [],
        "log_tail": [],
        "notes": [reason],
    })
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

/// The app's executable name from its forge.toml, or why it can't be read.
pub fn product(app_dir: &Path) -> Result<String, String> {
    let file = app_dir.join("forge.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let config =
        crate::config::ForgeConfig::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(crate::app_config::xtool_product(&config))
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
        assert_eq!(v["log_status"], "collected");
        assert_eq!(v["crash_reports"], "found");
        // The fake report can't be analysed: said, not hidden.
        assert_eq!(crashes[0]["symbolicated"], "unreadable");
    }

    /// A phone that logs nothing for the app and can stop waiting.
    struct Silent;
    static SILENT_ENDED: AtomicBool = AtomicBool::new(false);

    impl DeviceBackend for Silent {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Fake.devices()
        }
        fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
            Fake.info(device)
        }
        fn syslog(&self, _: &DeviceRef, _: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            unreachable!("the watch uses syslog_while")
        }
        fn syslog_while(
            &self,
            _: &DeviceRef,
            stop: &AtomicBool,
            _: &mut dyn FnMut(&str) -> bool,
        ) -> Result<()> {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            SILENT_ENDED.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn pull_crash_report(&self, _: &DeviceRef, _: &str) -> Result<Vec<u8>> {
            unreachable!("no new reports")
        }
    }

    fn silent() -> std::result::Result<Box<dyn DeviceBackend>, String> {
        Ok(Box::new(Silent))
    }

    #[test]
    fn a_quiet_app_is_empty_not_missing_and_the_reader_stops_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let watch = Watch::start(silent, "0000", "GoodDogXtool", tmp.path());
        let t = Instant::now();
        let done = watch.finish(tmp.path(), true);
        assert!(t.elapsed() < STOP_WAIT, "{:?}", t.elapsed());
        assert_eq!(done.value["log_status"], "empty");
        assert_eq!(done.value["crash_reports"], "none_new");
        assert_eq!(done.value["crashes_checked"], true);
        assert_eq!(done.value["notes"], json!([]));
    }

    /// A run that returned early (an error before `finish`): dropping the
    /// watch still stops the reader.
    #[test]
    fn dropping_an_unfinished_watch_stops_its_reader() {
        let tmp = tempfile::tempdir().unwrap();
        SILENT_ENDED.store(false, Ordering::SeqCst);
        let watch = Watch::start(silent, "0000", "GoodDogXtool", tmp.path());
        std::thread::sleep(Duration::from_millis(30));
        assert!(!SILENT_ENDED.load(Ordering::SeqCst));
        drop(watch);
        let deadline = Instant::now() + Duration::from_secs(1);
        while !SILENT_ENDED.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(SILENT_ENDED.load(Ordering::SeqCst));
    }

    /// A backend that only checks `stop` on a line, on a phone that sends
    /// none: finish gives up after STOP_WAIT and says the log is incomplete.
    struct Stuck;

    impl DeviceBackend for Stuck {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Fake.devices()
        }
        fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
            Fake.info(device)
        }
        fn syslog(&self, _: &DeviceRef, _: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            std::thread::sleep(STOP_WAIT + Duration::from_secs(1));
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn pull_crash_report(&self, _: &DeviceRef, _: &str) -> Result<Vec<u8>> {
            unreachable!("no new reports")
        }
    }

    fn stuck() -> std::result::Result<Box<dyn DeviceBackend>, String> {
        Ok(Box::new(Stuck))
    }

    #[test]
    fn a_reader_that_wont_stop_is_left_after_the_wait_and_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let watch = Watch::start(stuck, "0000", "GoodDogXtool", tmp.path());
        let t = Instant::now();
        let done = watch.finish(tmp.path(), false);
        assert!(t.elapsed() < STOP_WAIT + Duration::from_millis(500));
        assert_eq!(done.value["log_status"], "unavailable");
        assert!(done.value["notes"][0]
            .as_str()
            .unwrap()
            .contains("didn't stop"));
        assert_eq!(done.value["crash_reports"], "not_checked");
    }

    fn unplugged() -> std::result::Result<Box<dyn DeviceBackend>, String> {
        Err("usbmuxd: connection refused".into())
    }

    #[test]
    fn a_phone_that_cant_be_reached_is_unavailable_evidence_not_a_quiet_app() {
        let tmp = tempfile::tempdir().unwrap();
        let watch = Watch::start(unplugged, "0000", "GoodDogXtool", tmp.path());
        let done = watch.finish(tmp.path(), true);
        let v = &done.value;
        assert_eq!(v["log_status"], "unavailable");
        assert_eq!(v["crash_reports"], "unavailable");
        assert_eq!(v["crashes_checked"], false);
        let notes = v["notes"].to_string();
        assert!(notes.contains("app log not read: usbmuxd"), "{notes}");
        assert!(notes.contains("crash reports not checked"), "{notes}");
    }

    #[test]
    fn a_run_without_a_watch_says_why() {
        let v = not_watched("--no-watch");
        assert_eq!(v["status"], "not_watched");
        assert_eq!(v["log_status"], "unavailable");
        assert_eq!(v["crashes_checked"], false);
        assert!(product(Path::new("/nonexistent")).is_err());
    }
}
