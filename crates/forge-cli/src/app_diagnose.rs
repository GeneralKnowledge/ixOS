//! `forge diagnose`: why isn't my app working? One checklist for a person,
//! from what Forge can see on this computer and the phone: the last
//! `forge run ios`, whether the iPhone is there, whether the app is
//! installed and still signed, its newest crash report, and what it logs
//! while the person opens it. It ends with one next step.
//!
//! The agent-facing twin is MCP's `forge_diagnose` (crates/forge-mcp,
//! correlate.rs), which explains the last failed tool call in a session.
//! This one needs no session: it starts from the app folder.
//!
//! It only reports what it saw. A check it couldn't make (no phone, the
//! profiles unreadable) says so, and is never counted as passing.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use forge_device::{DeviceBackend, LogFilter};
use serde_json::{json, Value};

use crate::config::ForgeConfig;

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Ok,
    Warn,
    Fail,
    /// Not checked (no phone, or switched off).
    Skipped,
}

impl Mark {
    fn symbol(self) -> &'static str {
        match self {
            Mark::Ok => "✓",
            Mark::Warn => "!",
            Mark::Fail => "✗",
            Mark::Skipped => "-",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mark::Ok => "ok",
            Mark::Warn => "warning",
            Mark::Fail => "problem",
            Mark::Skipped => "skipped",
        }
    }
}

/// One line of the checklist, with the details under it and, when
/// something is wrong, what to do.
#[derive(Debug, Clone)]
pub struct Check {
    pub id: &'static str,
    pub mark: Mark,
    pub title: String,
    pub details: Vec<String>,
    pub next: Option<String>,
}

impl Check {
    fn new(id: &'static str, mark: Mark, title: impl Into<String>) -> Check {
        Check {
            id,
            mark,
            title: title.into(),
            details: Vec::new(),
            next: None,
        }
    }

    fn detail(mut self, line: impl Into<String>) -> Check {
        self.details.push(line.into());
        self
    }

    fn next(mut self, step: impl Into<String>) -> Check {
        self.next = Some(step.into());
        self
    }
}

/// What `forge diagnose` looks at.
pub struct Inputs<'a> {
    pub app_dir: &'a Path,
    pub cfg: &'a ForgeConfig,
    /// The phone connection, or why there is none (usbmuxd not running).
    pub backend: Result<&'a dyn DeviceBackend, String>,
    pub udid: Option<&'a str>,
    /// How long to read the app's log. Zero skips it.
    pub listen: Duration,
    /// Now, in seconds since 1970.
    pub now: i64,
}

/// The finished checklist.
pub struct Diagnosis {
    pub checks: Vec<Check>,
    /// The one thing to do next.
    pub next: String,
}

/// Most log lines shown; the rest are counted.
const LOG_LINES_SHOWN: usize = 12;
/// A profile with less than this many days left gets a warning.
const EXPIRY_WARN_DAYS: f64 = 1.0;

/// Runs every check. `say` gets the one line to show the person before the
/// log is read ("open the app now").
pub fn diagnose(inputs: &Inputs, say: &mut dyn FnMut(&str)) -> Diagnosis {
    let product = crate::app_config::xtool_product(inputs.cfg);
    let app_name = inputs
        .cfg
        .app
        .display_name
        .clone()
        .unwrap_or_else(|| inputs.cfg.app.name.clone());
    let mut checks = vec![last_run(inputs.app_dir, inputs.now)];

    match &inputs.backend {
        Err(e) => {
            checks.push(
                Check::new("iphone", Mark::Fail, "Can't reach the iPhone connection service")
                    .detail(e.clone())
                    .next("Start usbmuxd (`sudo systemctl start usbmuxd`), then run `forge diagnose` again. `forge doctor` checks the rest of the USB setup"),
            );
        }
        Ok(backend) => {
            let phone = phone(*backend, inputs.udid);
            let connected = phone.mark != Mark::Fail;
            checks.push(phone);
            if connected {
                checks.push(installed(
                    *backend,
                    inputs.udid,
                    &inputs.cfg.app.bundle_id,
                    inputs.now,
                ));
                checks.push(crashes(*backend, inputs.udid, &product, inputs.app_dir));
                if inputs.listen.is_zero() {
                    checks.push(Check::new(
                        "log",
                        Mark::Skipped,
                        "The app's log was not read (--for 0)",
                    ));
                } else {
                    say(&format!(
                        "Open {app_name} on the iPhone now. Reading its log for {} s…",
                        inputs.listen.as_secs()
                    ));
                    checks.push(log(*backend, inputs.udid, &product, inputs.listen));
                }
            }
        }
    }
    let next = next_step(&checks, &app_name);
    Diagnosis { checks, next }
}

/// The first problem's next step, else the first warning's, else what to
/// look at when everything Forge can see is fine.
fn next_step(checks: &[Check], app_name: &str) -> String {
    for mark in [Mark::Fail, Mark::Warn] {
        if let Some(step) = checks
            .iter()
            .filter(|c| c.mark == mark)
            .find_map(|c| c.next.clone())
        {
            return step;
        }
    }
    format!(
        "Nothing Forge can see is wrong with {app_name}. If the screen still looks wrong, `forge ui screenshot shot.png` and `forge ui tree` show what is on it (after `forge ui setup`, docs/forge-ui-testing.md)"
    )
}

/// `<app>/.forge/status/run-ios.json`, written by `forge run ios`.
fn last_run(app_dir: &Path, now: i64) -> Check {
    let path = app_dir.join(".forge/status/run-ios.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Check::new(
            "last_run",
            Mark::Warn,
            "No `forge run ios` recorded for this app",
        )
        .next("Build and install it with `forge run ios`");
    };
    let Ok(status) = serde_json::from_str::<Value>(&text) else {
        return Check::new(
            "last_run",
            Mark::Warn,
            format!("Couldn't read {}", path.display()),
        )
        .next("Run `forge run ios` again");
    };
    let phase = status["phase"].as_str().unwrap_or("unknown");
    let updated = status["updated"].as_i64().unwrap_or(0);
    let ago = ago(now - updated);
    let log = status["log"].as_str().map(|l| resolve(app_dir, l));
    match phase {
        "done" => Check::new(
            "last_run",
            Mark::Ok,
            format!("The last `forge run ios` installed the app ({ago})"),
        ),
        "failed" => {
            let error = status["error"].as_str().unwrap_or("");
            let output = log
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|t| tail(&t, 400))
                .unwrap_or_else(|| error.to_string());
            let hints = crate::diagnose::diagnose(&format!("{output}\n{error}"));
            let mut check = Check::new(
                "last_run",
                Mark::Fail,
                format!("The last `forge run ios` failed ({ago}), so the app on the phone is older than your code"),
            );
            if let Some(first) = error.lines().find(|l| !l.trim().is_empty()) {
                check = check.detail(first.trim().to_string());
            }
            for h in &hints {
                check = check.detail(format!("[{}] {}", h.stage.label(), h.problem));
            }
            if let Some(log) = &log {
                check = check.detail(format!("Full output: {}", log.display()));
            }
            match hints.first() {
                Some(h) => check.next(h.fix),
                None => check.next(
                    "Fix the error above (the full output names the file and line), then `forge run ios`",
                ),
            }
        }
        // Still beating: it is running now.
        _ if now - updated <= 2 * crate::status::BEAT.as_secs() as i64 => Check::new(
            "last_run",
            Mark::Warn,
            format!(
                "`forge run ios` is running now ({})",
                phase.replace('_', " ")
            ),
        )
        .next("Wait for `forge run ios` to finish, then run `forge diagnose` again"),
        _ => Check::new(
            "last_run",
            Mark::Warn,
            format!(
                "The last `forge run ios` stopped while {} ({ago}) and never finished",
                phase.replace('_', " ")
            ),
        )
        .next("Run `forge run ios` again"),
    }
}

fn resolve(app_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        app_dir.join(p)
    }
}

/// The last `lines` lines of `text`.
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn ago(secs: i64) -> String {
    match secs {
        s if s < 90 => "just now".into(),
        s if s < 90 * 60 => format!("{} minutes ago", s / 60),
        s if s < 36 * 3600 => format!("{} hours ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}

fn phone(backend: &dyn DeviceBackend, udid: Option<&str>) -> Check {
    let device = match forge_device::pick_device(backend, udid) {
        Ok(d) => d,
        Err(e) => {
            // On Linux usbmuxd runs only while a phone is plugged in, so
            // "can't reach usbmuxd" nearly always just means "unplugged":
            // don't send someone to install a package they already have.
            let e = e.to_string();
            let detail = if e.contains("usbmuxd") {
                "Nothing is answering on USB. On Linux that usually just means no iPhone is plugged in; if one is plugged in and unlocked, `forge doctor` checks the USB setup".to_string()
            } else {
                e
            };
            return Check::new("iphone", Mark::Fail, "No iPhone found")
                .detail(detail)
                .next("Connect the iPhone with a cable, unlock it and tap Trust if it asks, then run `forge diagnose` again");
        }
    };
    crate::report::set_device(&device.udid, None);
    match backend.info(&device) {
        Ok(info) if info.developer_mode == Some(false) => Check::new(
            "iphone",
            Mark::Fail,
            format!("{} is connected, but Developer Mode is off", info.name),
        )
        .next("On the iPhone: Settings > Privacy & Security > Developer Mode, turn it on and restart the phone"),
        Ok(info) => Check::new(
            "iphone",
            Mark::Ok,
            format!("{} is connected (iOS {})", info.name, info.ios_version),
        ),
        // Locked phones refuse some services; the rest may still work.
        Err(e) => Check::new("iphone", Mark::Ok, "An iPhone is connected")
            .detail(format!("Its details couldn't be read ({e}); unlock it if the checks below fail")),
    }
}

fn installed(backend: &dyn DeviceBackend, udid: Option<&str>, bundle_id: &str, now: i64) -> Check {
    let data = match crate::device::apps_data(backend, udid, now, true) {
        Ok(d) => d,
        Err(e) => {
            return Check::new(
                "installed",
                Mark::Skipped,
                "Couldn't list the apps on the phone",
            )
            .detail(e)
            .next("Unlock the iPhone and run `forge diagnose` again")
        }
    };
    let apps = data["apps"].as_array().cloned().unwrap_or_default();
    let Some(app) = apps
        .iter()
        .find(|a| a["original_id"].as_str() == Some(bundle_id))
    else {
        let slots = &data["free_team_slots"];
        let mut check = Check::new(
            "installed",
            Mark::Fail,
            format!("{bundle_id} is not installed on this iPhone"),
        );
        if slots["free"].as_u64() == Some(0) {
            check = check
                .detail("All 3 free-team app slots are in use (`forge device apps` lists them)")
                .next("Remove one of your other development apps from the iPhone, then `forge run ios`");
        } else {
            check = check.next("Install it with `forge run ios`");
        }
        return check;
    };
    let days = app["days_left"].as_f64();
    match (app["expired"].as_bool(), days) {
        (Some(true), _) => Check::new(
            "installed",
            Mark::Fail,
            "The app is installed, but its signing has expired, so iOS won't open it",
        )
        .detail("Free Apple accounts sign apps for 7 days")
        .next("Run `forge run ios` to sign and install it again (your data on the phone is kept)"),
        (_, Some(d)) if d < EXPIRY_WARN_DAYS => Check::new(
            "installed",
            Mark::Warn,
            format!(
                "The app is installed, but its signing expires in {:.0} hours",
                d * 24.0
            ),
        )
        .next("Run `forge run ios` soon to sign it for another 7 days"),
        (_, Some(d)) => Check::new(
            "installed",
            Mark::Ok,
            format!("The app is installed; its signing has {d:.1} days left"),
        ),
        _ => {
            let mut check = Check::new("installed", Mark::Ok, "The app is installed");
            if let Some(e) = data["profiles_error"].as_str() {
                check = check.detail(format!("How long its signing lasts is unknown ({e})"));
            }
            check
        }
    }
}

fn crashes(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    product: &str,
    app_dir: &Path,
) -> Check {
    let listed = forge_device::pick_device(backend, udid)
        .and_then(|d| backend.crash_reports(&d).map(|names| (d, names)));
    let (device, names) = match listed {
        Ok(x) => x,
        Err(e) => {
            return Check::new(
                "crashes",
                Mark::Skipped,
                "Couldn't list the phone's crash reports",
            )
            .detail(e.to_string())
        }
    };
    let mine = forge_device::app_crash_reports(&names, product);
    let Some(newest) = mine.first() else {
        return Check::new(
            "crashes",
            Mark::Ok,
            format!("No crash reports for {product} on the phone"),
        );
    };
    let file = newest.rsplit('/').next().unwrap_or(newest).to_string();
    let mut check = Check::new(
        "crashes",
        Mark::Warn,
        format!(
            "{} crash report(s) for {product}; the newest is {file}",
            mine.len()
        ),
    )
    .detail("Check its date: a crash from before your last install may already be fixed");
    let dir = app_dir.join(".forge/diagnose/crashes");
    let pulled = backend
        .pull_crash_report(&device, newest)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(&file);
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(path)
        });
    let path = match pulled {
        Ok(p) => p,
        Err(e) => {
            return check
                .detail(format!("It couldn't be copied ({e})"))
                .next(format!(
                    "Read it with `forge device crashes --app {product} --pull crashes/`"
                ))
        }
    };
    let symbols = crate::crash::options(Some(app_dir), &[], false);
    match crate::crash::analyze_pulled(std::slice::from_ref(&path), &symbols)
        .into_iter()
        .next()
        .map(|(_, a)| a)
    {
        Some(Ok(a)) => {
            for line in crate::crash::summary(&a).lines() {
                check = check.detail(line.trim_end().to_string());
            }
            check
                .detail(format!("Full report: {}.txt", path.display()))
                .next("Fix the crash where the report points (the first frame in your own code), then `forge run ios`")
        }
        Some(Err(crate::crash::Failure::NotACrash(kind))) => Check::new(
            "crashes",
            Mark::Ok,
            format!("No crashes for {product}; the newest report ({file}) is a {kind} report, not a crash"),
        ),
        Some(Err(e)) => check
            .detail(format!("Copied to {}, but not read: {e}", path.display()))
            .next(format!("Read it with `forge crash {}`", path.display())),
        None => check
            .detail(format!("Copied to {}", path.display()))
            .next(format!("Read it with `forge crash {}`", path.display())),
    }
}

fn log(backend: &dyn DeviceBackend, udid: Option<&str>, product: &str, listen: Duration) -> Check {
    let device = match forge_device::pick_device(backend, udid) {
        Ok(d) => d,
        Err(e) => {
            return Check::new("log", Mark::Skipped, "The app's log was not read")
                .detail(e.to_string())
        }
    };
    let filter = LogFilter {
        process: Some(product.to_string()),
        contains: None,
    };
    let until = std::time::Instant::now() + listen;
    let mut lines: Vec<String> = Vec::new();
    let read = backend.syslog_until(&device, Some(until), &mut |line| {
        if filter.matches(line) {
            lines.push(line.to_string());
        }
        true
    });
    if let Err(e) = read {
        if lines.is_empty() {
            return Check::new("log", Mark::Skipped, "The app's log couldn't be read")
                .detail(e.to_string());
        }
    }
    let secs = listen.as_secs();
    if lines.is_empty() {
        return Check::new(
            "log",
            Mark::Warn,
            format!("{product} wrote nothing to the log in {secs} s"),
        )
        .detail("Either it wasn't opened, or it closed before it could log anything")
        .next(format!(
            "Open the app on the iPhone while `forge device logs --process {product} --for 30s` runs. If it closes at once, see docs/troubleshooting.md (\"The app opens to a blank screen, or closes straight away\")"
        ));
    }
    let errors: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains("<Error>") || l.contains("<Fault>"))
        .collect();
    let mut check = if errors.is_empty() {
        Check::new(
            "log",
            Mark::Ok,
            format!(
                "{product} logged {} line(s) in {secs} s, none of them errors",
                lines.len()
            ),
        )
    } else {
        Check::new(
            "log",
            Mark::Warn,
            format!(
                "{product} logged {} line(s) in {secs} s, {} of them errors",
                lines.len(),
                errors.len()
            ),
        )
        .next(format!(
            "Read the errors above; `forge device logs --process {product} --for 60s` shows the log live while you use the app"
        ))
    };
    let shown: Vec<&String> = if errors.is_empty() {
        let start = lines.len().saturating_sub(LOG_LINES_SHOWN);
        lines[start..].iter().collect()
    } else {
        let start = errors.len().saturating_sub(LOG_LINES_SHOWN);
        errors[start..].to_vec()
    };
    for line in shown {
        check = check.detail(line.clone());
    }
    check
}

/// The checklist as text.
pub fn print(d: &Diagnosis, out: &mut dyn Write) -> std::io::Result<()> {
    for c in &d.checks {
        writeln!(out, "  {} {}", c.mark.symbol(), c.title)?;
        for line in &c.details {
            writeln!(out, "      {line}")?;
        }
    }
    writeln!(out)?;
    writeln!(out, "Next: {}", d.next)
}

/// The checklist for `--json` (`data` of the forge.result/1 object).
pub fn to_json(d: &Diagnosis) -> Value {
    json!({
        "checks": d.checks.iter().map(|c| json!({
            "id": c.id,
            "result": c.mark.name(),
            "title": c.title,
            "details": c.details,
            "next": c.next,
        })).collect::<Vec<_>>(),
        "next": d.next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_device::{Connection, DeviceError, DeviceInfo, DeviceRef, InstalledApp, Result};

    const TOML: &str = "[app]\nname = \"Shopping\"\nbundle_id = \"com.example.shopping\"\nminimum_ios = \"17.0\"\n\n[rust]\npath = \"rust\"\n\n[build]\nios_device = true\nios_simulator = false\n\n[ios]\nxtool_product = \"ShoppingXtool\"\n";

    fn cfg() -> ForgeConfig {
        ForgeConfig::parse(TOML).unwrap()
    }

    #[derive(Default)]
    struct Phone {
        absent: bool,
        developer_mode: Option<bool>,
        apps: Vec<&'static str>,
        crashes: Vec<&'static str>,
        log: Vec<&'static str>,
    }

    fn device() -> DeviceRef {
        DeviceRef {
            udid: "00008110-TEST".into(),
            connection: Connection::Usb,
        }
    }

    impl DeviceBackend for Phone {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Ok(if self.absent { vec![] } else { vec![device()] })
        }
        fn info(&self, d: &DeviceRef) -> Result<DeviceInfo> {
            Ok(DeviceInfo {
                udid: d.udid.clone(),
                connection: Connection::Usb,
                name: "Test iPhone".into(),
                product_type: "iPhone16,2".into(),
                ios_version: "26.3.1".into(),
                build_version: "23D".into(),
                developer_mode: self.developer_mode.or(Some(true)),
            })
        }
        fn syslog(&self, _: &DeviceRef, on_line: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            for line in &self.log {
                if !on_line(line) {
                    break;
                }
            }
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            Ok(self.crashes.iter().map(|s| s.to_string()).collect())
        }
        fn pull_crash_report(&self, _: &DeviceRef, _: &str) -> Result<Vec<u8>> {
            Err(DeviceError::Service("afc error: Unknown error".into()))
        }
        fn user_apps(&self, _: &DeviceRef) -> Result<Vec<InstalledApp>> {
            Ok(self
                .apps
                .iter()
                .map(|id| InstalledApp {
                    bundle_id: id.to_string(),
                    name: None,
                    version: None,
                    executable: None,
                })
                .collect())
        }
        fn provisioning_profiles(&self, _: &DeviceRef) -> Result<Vec<Vec<u8>>> {
            Ok(vec![])
        }
    }

    fn run(dir: &Path, phone: &Phone, listen: u64) -> Diagnosis {
        let cfg = cfg();
        diagnose(
            &Inputs {
                app_dir: dir,
                cfg: &cfg,
                backend: Ok(phone),
                udid: None,
                listen: Duration::from_secs(listen),
                now: 10_000,
            },
            &mut |_| {},
        )
    }

    fn status(dir: &Path, doc: Value) {
        std::fs::create_dir_all(dir.join(".forge/status")).unwrap();
        std::fs::write(dir.join(".forge/status/run-ios.json"), doc.to_string()).unwrap();
    }

    fn mark(d: &Diagnosis, id: &str) -> Mark {
        d.checks.iter().find(|c| c.id == id).unwrap().mark
    }

    fn healthy() -> Phone {
        Phone {
            apps: vec!["XTL-ND4P87PX52.com.example.shopping"],
            log: vec![
                "Nov  9 08:00 iPhone ShoppingXtool[7] <Notice>: opened",
                "Nov  9 08:00 iPhone Mail[2] <Notice>: fetched",
            ],
            ..Phone::default()
        }
    }

    #[test]
    fn a_healthy_app_ends_with_where_to_look_next() {
        let dir = tempfile::tempdir().unwrap();
        status(dir.path(), json!({"phase": "done", "updated": 9_000}));
        let d = run(dir.path(), &healthy(), 1);
        for id in ["last_run", "iphone", "installed", "crashes", "log"] {
            assert_eq!(mark(&d, id), Mark::Ok, "{id}: {:?}", d.checks);
        }
        assert!(
            d.next
                .contains("Nothing Forge can see is wrong with Shopping"),
            "{}",
            d.next
        );
        let mut text = Vec::new();
        print(&d, &mut text).unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(
            text.contains("✓ The last `forge run ios` installed the app (16 minutes ago)"),
            "{text}"
        );
        assert!(text.contains("ShoppingXtool logged 1 line(s)"), "{text}");
        assert!(!text.contains("Mail"), "{text}");
    }

    #[test]
    fn a_failed_run_comes_first_with_its_fix() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("run.log");
        std::fs::write(&log, "error: requires logging in with `xtool auth`\n").unwrap();
        status(
            dir.path(),
            json!({"phase": "failed", "updated": 9_900, "error": "`xtool dev run` failed (exit status: 1)", "log": log}),
        );
        let d = run(dir.path(), &healthy(), 0);
        assert_eq!(mark(&d, "last_run"), Mark::Fail);
        assert!(d.next.contains("xtool auth login"), "{}", d.next);
        assert_eq!(mark(&d, "log"), Mark::Skipped);
    }

    #[test]
    fn no_phone_says_so_and_checks_nothing_on_it() {
        let dir = tempfile::tempdir().unwrap();
        status(dir.path(), json!({"phase": "done", "updated": 9_000}));
        let d = run(
            dir.path(),
            &Phone {
                absent: true,
                ..Phone::default()
            },
            1,
        );
        assert_eq!(mark(&d, "iphone"), Mark::Fail);
        assert_eq!(d.checks.len(), 2, "{:?}", d.checks);
        assert!(d.next.contains("Connect the iPhone"), "{}", d.next);
    }

    #[test]
    fn developer_mode_off_is_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone {
            developer_mode: Some(false),
            ..healthy()
        };
        let d = run(dir.path(), &phone, 0);
        assert_eq!(mark(&d, "iphone"), Mark::Fail);
        assert!(d.next.contains("Developer Mode"), "{}", d.next);
    }

    #[test]
    fn a_missing_app_says_to_install_it() {
        let dir = tempfile::tempdir().unwrap();
        status(dir.path(), json!({"phase": "done", "updated": 9_000}));
        let d = run(dir.path(), &Phone::default(), 0);
        assert_eq!(mark(&d, "installed"), Mark::Fail);
        assert!(d.next.contains("forge run ios"), "{}", d.next);
    }

    #[test]
    fn a_crash_and_a_silent_app_are_warnings() {
        let dir = tempfile::tempdir().unwrap();
        status(dir.path(), json!({"phase": "done", "updated": 9_000}));
        let phone = Phone {
            crashes: vec![
                "ShoppingXtool-2026-11-08-101500.ips",
                "ShoppingXtool-2026-11-09-080000.ips",
                "Mail-2026-11-09-080000.ips",
            ],
            log: vec!["Nov  9 08:00 iPhone Mail[2] <Notice>: fetched"],
            ..healthy()
        };
        let d = run(dir.path(), &phone, 1);
        let crash = d.checks.iter().find(|c| c.id == "crashes").unwrap();
        assert_eq!(crash.mark, Mark::Warn);
        assert!(crash.title.contains("2 crash report(s)"), "{}", crash.title);
        assert!(crash.title.contains("2026-11-09-080000"), "{}", crash.title);
        assert_eq!(mark(&d, "log"), Mark::Warn);
        // The crash comes before the silent log.
        assert!(d.next.contains("forge device crashes"), "{}", d.next);
    }

    #[test]
    fn error_lines_are_shown_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        status(dir.path(), json!({"phase": "done", "updated": 9_000}));
        let phone = Phone {
            log: vec![
                "Nov  9 08:00 iPhone ShoppingXtool[7] <Notice>: opened",
                "Nov  9 08:00 iPhone ShoppingXtool[7] <Error>: storage error: denied",
            ],
            ..healthy()
        };
        let d = run(dir.path(), &phone, 1);
        let log = d.checks.iter().find(|c| c.id == "log").unwrap();
        assert_eq!(log.mark, Mark::Warn);
        assert!(log.title.contains("1 of them errors"), "{}", log.title);
        assert_eq!(log.details.len(), 1);
        assert!(log.details[0].contains("storage error"));
    }

    #[test]
    fn an_interrupted_or_missing_run_is_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let d = run(dir.path(), &healthy(), 0);
        assert_eq!(mark(&d, "last_run"), Mark::Warn);
        status(dir.path(), json!({"phase": "building", "updated": 5_000}));
        let d = run(dir.path(), &healthy(), 0);
        let c = &d.checks[0];
        assert!(c.title.contains("stopped while building"), "{}", c.title);
        status(
            dir.path(),
            json!({"phase": "waiting_for_device", "updated": 9_995}),
        );
        let d = run(dir.path(), &healthy(), 0);
        assert!(d.checks[0]
            .title
            .contains("running now (waiting for device)"));
    }

    #[test]
    fn no_usbmuxd_is_reported_without_a_phone_check() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = cfg();
        let d = diagnose(
            &Inputs {
                app_dir: dir.path(),
                cfg: &cfg,
                backend: Err("cannot connect to usbmuxd".into()),
                udid: None,
                listen: Duration::from_secs(1),
                now: 10_000,
            },
            &mut |_| {},
        );
        assert_eq!(d.checks.len(), 2);
        assert!(d.next.contains("usbmuxd"), "{}", d.next);
        let v = to_json(&d);
        assert_eq!(v["checks"][1]["result"], "problem");
    }
}
