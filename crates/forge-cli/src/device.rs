//! `forge device`: a connected iPhone's info, live logs and crash reports,
//! from Linux over usbmuxd (Forge Device M0). The phone work is in the
//! `forge-device` crate; this file only formats it.

use std::io::Write;
use std::path::Path;

use forge_device::{app_crash_reports, pick_device, DeviceBackend, DeviceInfo, LogFilter};
use serde_json::{json, Value};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// `forge device list`
pub fn list(backend: &dyn DeviceBackend, out: &mut dyn Write) -> Result<(), String> {
    let devices = backend.devices().map_err(err)?;
    if devices.is_empty() {
        return Err(err(forge_device::DeviceError::NoDevice));
    }
    for device in devices {
        let info = backend.info(&device);
        match info {
            Ok(info) => writeln!(
                out,
                "{}  {}  {} iOS {}  ({})",
                info.udid, info.name, info.product_type, info.ios_version, info.connection
            ),
            Err(e) => writeln!(
                out,
                "{}  ({})  details unavailable: {e}",
                device.udid, device.connection
            ),
        }
        .map_err(err)?;
    }
    Ok(())
}

fn info_json(info: &DeviceInfo) -> Value {
    json!({
        "udid": info.udid,
        "name": info.name,
        "model": info.product_type,
        "ios": info.ios_version,
        "build": info.build_version,
        "connection": info.connection.to_string(),
        "developer_mode": info.developer_mode,
    })
}

/// `forge device list --json`: `{"devices": [...]}`.
pub fn list_data(backend: &dyn DeviceBackend) -> Result<Value, String> {
    let devices = backend.devices().map_err(err)?;
    if devices.is_empty() {
        return Err(err(forge_device::DeviceError::NoDevice));
    }
    let list: Vec<Value> = devices
        .iter()
        .map(|device| match backend.info(device) {
            Ok(info) => info_json(&info),
            Err(e) => json!({
                "udid": device.udid,
                "connection": device.connection.to_string(),
                "error": e.to_string(),
            }),
        })
        .collect();
    Ok(json!({ "devices": list }))
}

/// `forge device info --json`: the phone's details; also names the phone in
/// the result's `device`.
pub fn info_data(backend: &dyn DeviceBackend, udid: Option<&str>) -> Result<Value, String> {
    let device = pick_device(backend, udid).map_err(err)?;
    let info = backend.info(&device).map_err(err)?;
    crate::report::set_device(&info.udid, Some(&info.name));
    Ok(info_json(&info))
}

/// `forge device info`
pub fn info(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let device = pick_device(backend, udid).map_err(err)?;
    let info = backend.info(&device).map_err(err)?;
    let developer_mode = match info.developer_mode {
        Some(true) => "on",
        Some(false) => "off (Settings > Privacy & Security > Developer Mode)",
        None => "unknown",
    };
    writeln!(
        out,
        "Name:           {}\nModel:          {}\niOS:            {} ({})\nUDID:           {}\nConnection:     {}\nDeveloper Mode: {}",
        info.name,
        info.product_type,
        info.ios_version,
        info.build_version,
        info.udid,
        info.connection,
        developer_mode
    )
    .map_err(err)
}

/// A free Apple team can have this many of its apps on a phone at once.
pub const FREE_TEAM_SLOTS: usize = 3;

/// `forge device apps --json`: each app you installed with its profile's
/// team, lifetime and days left, and the free-team slots in use. Apps with
/// no provisioning profile (the App Store's) are left out unless `all`.
pub fn apps_data(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    now: i64,
    all: bool,
) -> Result<Value, String> {
    let device = pick_device(backend, udid).map_err(err)?;
    crate::report::set_device(&device.udid, None);
    let apps = backend.user_apps(&device).map_err(err)?;
    // Profiles are best effort: an app list without expiry still helps.
    let (profiles, profile_error) = match backend.provisioning_profiles(&device) {
        Ok(ders) => (
            ders.iter()
                .filter_map(|d| forge_device::profile_from_der(d))
                .collect::<Vec<_>>(),
            None,
        ),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let rows: Vec<Value> = apps
        .iter()
        .map(|app| {
            // The newest profile that signs this bundle id (exactly, or by
            // a wildcard App ID).
            let profile = profiles
                .iter()
                .filter(|p| {
                    p.bundle_id().is_some_and(|id| {
                        id == app.bundle_id
                            || id == "*"
                            || id
                                .strip_suffix('*')
                                .is_some_and(|pre| app.bundle_id.starts_with(pre))
                    })
                })
                .max_by_key(|p| p.expires.unwrap_or(0));
            let free = profile.and_then(|p| p.lifetime_days()).map(|d| d <= 7);
            json!({
                "bundle_id": app.bundle_id,
                "original_id": forge_uitest::policy::original_id(&app.bundle_id),
                "name": app.name,
                "version": app.version,
                "executable": app.executable,
                "sideloaded": app.bundle_id.starts_with("XTL-"),
                "team": profile.and_then(|p| p.team.clone()),
                "free_team": free,
                "expires": profile.and_then(|p| p.expires),
                "days_left": profile.and_then(|p| p.expires).map(|e| (e - now) as f64 / 86_400.0),
                "expired": profile.and_then(|p| p.expires).map(|e| e <= now),
            })
        })
        .collect();
    let (rows, without_profile): (Vec<Value>, Vec<Value>) =
        rows.into_iter().partition(|r| all || !r["team"].is_null());
    let free_in_use = rows
        .iter()
        .filter(|r| r["free_team"] == json!(true) && r["expired"] != json!(true))
        .count();
    let mut data = json!({
        "apps": rows,
        "free_team_slots": { "limit": FREE_TEAM_SLOTS, "in_use": free_in_use, "free": FREE_TEAM_SLOTS.saturating_sub(free_in_use) },
    });
    if !all {
        data["not_shown_without_profile"] = json!(without_profile.len());
    }
    if let Some(e) = profile_error {
        data["profiles_error"] = json!(e);
    }
    Ok(data)
}

/// `forge device apps`
pub fn apps(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    now: i64,
    all: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let data = apps_data(backend, udid, now, all)?;
    let apps = data["apps"].as_array().cloned().unwrap_or_default();
    if apps.is_empty() {
        writeln!(out, "No apps you installed are on the phone.").map_err(err)?;
    }
    for app in &apps {
        let expiry = match (app["days_left"].as_f64(), app["expired"].as_bool()) {
            (_, Some(true)) => "profile expired: reinstall to use it".to_string(),
            (Some(d), _) => format!("expires in {d:.1} days"),
            _ => "no profile found".to_string(),
        };
        let team = match (app["team"].as_str(), app["free_team"].as_bool()) {
            (Some(t), Some(true)) => format!("{t}, free"),
            (Some(t), _) => t.to_string(),
            _ => "?".into(),
        };
        writeln!(
            out,
            "{:<44} {:<16} {:<18} {}",
            app["original_id"].as_str().unwrap_or(""),
            app["name"].as_str().unwrap_or(""),
            team,
            expiry
        )
        .map_err(err)?;
    }
    if let Some(n) = data["not_shown_without_profile"]
        .as_u64()
        .filter(|n| *n > 0)
    {
        writeln!(out, "{n} app(s) with no provisioning profile (from the App Store) not shown; --all lists them.").map_err(err)?;
    }
    let slots = &data["free_team_slots"];
    writeln!(
        out,
        "Free-team slots: {} of {} in use. A new free-team app needs a free slot: remove one on the phone first.",
        slots["in_use"], slots["limit"]
    )
    .map_err(err)?;
    if let Some(e) = data["profiles_error"].as_str() {
        writeln!(out, "Profiles not read ({e}): expiry unknown").map_err(err)?;
    }
    Ok(())
}

/// `forge device logs`: streams until the phone disconnects, `limit` lines
/// have matched, `duration` has passed, or the user presses Ctrl-C.
pub fn logs(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    filter: &LogFilter,
    limit: Option<usize>,
    duration: Option<std::time::Duration>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let device = pick_device(backend, udid).map_err(err)?;
    crate::report::set_device(&device.udid, None);
    let until = duration.map(|d| std::time::Instant::now() + d);
    let mut shown = 0usize;
    let mut write_failed = None;
    backend
        .syslog_until(&device, until, &mut |line| {
            if !filter.matches(line) {
                return true;
            }
            if let Err(e) = writeln!(out, "{line}").and_then(|_| out.flush()) {
                // The reader went away (`| head`): stop quietly.
                write_failed = Some(e);
                return false;
            }
            shown += 1;
            limit.is_none_or(|limit| shown < limit)
        })
        .map_err(err)?;
    match write_failed {
        Some(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(err(e)),
        _ => Ok(()),
    }
}

/// `forge device crashes`: lists an app's crash reports (all reports
/// without `app`), and with `pull` copies them into that folder.
pub fn crashes(
    backend: &dyn DeviceBackend,
    query: &CrashQuery,
    out: &mut dyn Write,
) -> Result<(), String> {
    let CrashQuery {
        udid, app, pull, ..
    } = *query;
    let (device, names) = crash_names(backend, udid, app, query.limit)?;
    if names.is_empty() {
        writeln!(
            out,
            "No crash reports{} on the phone.",
            app.map(|a| format!(" for {a}")).unwrap_or_default()
        )
        .map_err(err)?;
        return Ok(());
    }
    match pull {
        None => {
            for name in &names {
                writeln!(out, "{name}").map_err(err)?;
            }
        }
        Some(dir) => {
            let (files, skipped) = pull_reports(backend, &device, &names, dir)?;
            for (name, e) in &skipped {
                writeln!(out, "not copied: {name}: {e}").map_err(err)?;
            }
            let analyses = crate::crash::analyze_pulled(&files, &query.symbols());
            for file in &files {
                writeln!(out, "{}", file.display()).map_err(err)?;
                match analyses.iter().find(|(f, _)| f == file).map(|(_, a)| a) {
                    Some(Ok(a)) => {
                        write!(out, "{}", crate::crash::summary(a)).map_err(err)?;
                        writeln!(out, "  (full report: {}.txt)", file.display()).map_err(err)?;
                    }
                    Some(Err(e @ crate::crash::Failure::NotACrash(_))) => {
                        writeln!(out, "  {e}: skipped").map_err(err)?
                    }
                    Some(Err(e)) => writeln!(out, "  not symbolicated: {e}").map_err(err)?,
                    None => {}
                }
            }
        }
    }
    Ok(())
}

/// What `forge device crashes` lists and pulls.
#[derive(Debug, Clone, Copy, Default)]
pub struct CrashQuery<'a> {
    pub udid: Option<&'a str>,
    /// Only this app's reports (its executable name).
    pub app: Option<&'a str>,
    /// Copy the reports here, and symbolicate them.
    pub pull: Option<&'a Path>,
    /// Only the newest N.
    pub limit: Option<usize>,
    /// The app folder whose build the reports are symbolicated against.
    pub project: Option<&'a Path>,
}

impl CrashQuery<'_> {
    fn symbols(&self) -> forge_crash::Options {
        crate::crash::options(self.project, &[], false)
    }
}

/// `forge device crashes --json`: `{"reports": [...]}`, plus `"pulled"`
/// (local paths) and `"crashes"` (each report symbolicated: kind, message,
/// likely cause, crashed thread; see forge-crash's `Analysis`) with `--pull`.
pub fn crashes_data(backend: &dyn DeviceBackend, query: &CrashQuery) -> Result<Value, String> {
    let (device, names) = crash_names(backend, query.udid, query.app, query.limit)?;
    let mut data = json!({ "reports": names });
    if let Some(dir) = query.pull {
        let (files, skipped) = pull_reports(backend, &device, &names, dir)?;
        if !skipped.is_empty() {
            data["skipped"] = json!(skipped
                .iter()
                .map(|(name, e)| json!({ "name": name, "error": e }))
                .collect::<Vec<_>>());
        }
        data["pulled"] = json!(files
            .iter()
            .map(|f| f.display().to_string())
            .collect::<Vec<_>>());
        let analyses = crate::crash::analyze_pulled(&files, &query.symbols());
        // Resource and hang reports sit next to crashes on the phone; they
        // are listed apart so `crashes` holds crashes and real failures only.
        let (other, crashes): (Vec<_>, Vec<_>) = analyses
            .iter()
            .partition(|(_, a)| matches!(a, Err(crate::crash::Failure::NotACrash(_))));
        data["crashes"] = json!(crashes
            .iter()
            .map(|(f, a)| crate::crash::entry(f, a))
            .collect::<Vec<_>>());
        if !other.is_empty() {
            data["not_crashes"] = json!(other
                .iter()
                .map(|(f, a)| {
                    let bug = match a {
                        Err(crate::crash::Failure::NotACrash(bug)) => bug.as_str(),
                        _ => "",
                    };
                    json!({ "file": f.display().to_string(), "bug_type": bug })
                })
                .collect::<Vec<_>>());
        }
    }
    Ok(data)
}

fn crash_names(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    app: Option<&str>,
    limit: Option<usize>,
) -> Result<(forge_device::DeviceRef, Vec<String>), String> {
    let device = pick_device(backend, udid).map_err(err)?;
    crate::report::set_device(&device.udid, None);
    let all = backend.crash_reports(&device).map_err(err)?;
    let names = match app {
        Some(app) => app_crash_reports(&all, app),
        None => {
            let mut all = all;
            all.sort();
            all
        }
    };
    let names = match limit {
        Some(n) => names.into_iter().take(n).collect(),
        None => names,
    };
    Ok((device, names))
}

/// Reports copied, and the ones the phone wouldn't give (`name`, reason).
/// One unreadable entry (a folder, a report being written) doesn't stop the
/// rest; seen on Cati, 2026-11-08, as `afc error: Unknown error`.
type Pulled = (Vec<std::path::PathBuf>, Vec<(String, String)>);

fn pull_reports(
    backend: &dyn DeviceBackend,
    device: &forge_device::DeviceRef,
    names: &[String],
    dir: &Path,
) -> Result<Pulled, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for name in names {
        let bytes = match backend.pull_crash_report(device, name) {
            Ok(bytes) => bytes,
            Err(e) => {
                skipped.push((name.clone(), e.to_string()));
                continue;
            }
        };
        let file = dir.join(name.rsplit('/').next().unwrap_or(name));
        std::fs::write(&file, bytes).map_err(|e| format!("write {}: {e}", file.display()))?;
        files.push(file);
    }
    if files.is_empty() {
        if let Some((name, e)) = skipped.first() {
            return Err(format!("could not copy any report ({name}: {e})"));
        }
    }
    Ok((files, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_device::{Connection, DeviceInfo, DeviceRef, Result};

    struct Fake;

    fn phone() -> DeviceRef {
        DeviceRef {
            udid: "00008110-TEST".into(),
            connection: Connection::Usb,
        }
    }

    impl DeviceBackend for Fake {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Ok(vec![phone()])
        }
        fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
            Ok(DeviceInfo {
                udid: device.udid.clone(),
                connection: Connection::Usb,
                name: "Test iPhone".into(),
                product_type: "iPhone15,2".into(),
                ios_version: "18.1".into(),
                build_version: "22B83".into(),
                developer_mode: Some(false),
            })
        }
        fn syslog(&self, _: &DeviceRef, on_line: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            for line in [
                "Nov  7 12:00 iPhone Chat[1] <Notice>: [chat] opened",
                "Nov  7 12:00 iPhone Mail[2] <Notice>: fetched",
                "Nov  7 12:01 iPhone Chat[1] <Notice>: [chat] sent",
                "Nov  7 12:02 iPhone Chat[1] <Notice>: [chat] received",
            ] {
                if !on_line(line) {
                    break;
                }
            }
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            Ok(vec![
                "Chat-2026-11-07-120000.ips".into(),
                "Mail-2026-11-07-120000.ips".into(),
                "Crashy-2026-11-08-101500.ips".into(),
                "Safari-2026-11-08-100000.ips".into(),
            ])
        }
        fn pull_crash_report(&self, _: &DeviceRef, name: &str) -> Result<Vec<u8>> {
            if name.starts_with("Mail") {
                return Err(forge_device::DeviceError::Service(
                    "afc error: Unknown error".into(),
                ));
            }
            if name.starts_with("Crashy") {
                return Ok(std::fs::read(crash_fixtures().join(name)).unwrap());
            }
            if name.starts_with("Safari") {
                return Ok(b"{\"bug_type\":\"145\"}\n{}".to_vec());
            }
            Ok(format!("report {name}").into_bytes())
        }
    }

    struct Apps;

    fn profile(app_id: &str, created: &str, expires: &str) -> Vec<u8> {
        format!(
            "\u{30}garbage<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
             <key>Name</key><string>p</string>\
             <key>TeamIdentifier</key><array><string>ND4P87PX52</string></array>\
             <key>CreationDate</key><date>{created}</date>\
             <key>ExpirationDate</key><date>{expires}</date>\
             <key>Entitlements</key><dict><key>application-identifier</key><string>ND4P87PX52.{app_id}</string></dict>\
             </dict></plist>tail"
        )
        .into_bytes()
    }

    impl DeviceBackend for Apps {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Ok(vec![phone()])
        }
        fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
            Fake.info(device)
        }
        fn syslog(&self, _: &DeviceRef, _: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn pull_crash_report(&self, _: &DeviceRef, _: &str) -> Result<Vec<u8>> {
            Ok(vec![])
        }
        fn user_apps(&self, _: &DeviceRef) -> Result<Vec<forge_device::InstalledApp>> {
            let app = |id: &str, name: &str| forge_device::InstalledApp {
                bundle_id: id.into(),
                name: Some(name.into()),
                version: Some("1.0".into()),
                executable: None,
            };
            Ok(vec![
                app("XTL-ND4P87PX52.org.forgemobile.gooddog", "Good Dog"),
                app(
                    "XTL-ND4P87PX52.org.forgemobile.pocketlibrary",
                    "Pocket Library",
                ),
                app("com.example.store", "From the App Store"),
            ])
        }
        fn provisioning_profiles(&self, _: &DeviceRef) -> Result<Vec<Vec<u8>>> {
            Ok(vec![
                profile(
                    "XTL-ND4P87PX52.org.forgemobile.gooddog",
                    "2026-11-08T10:00:00Z",
                    "2026-11-15T10:00:00Z",
                ),
                // Pocket Library's profile ran out.
                profile(
                    "XTL-ND4P87PX52.org.forgemobile.pocketlibrary",
                    "2026-10-30T10:00:00Z",
                    "2026-11-06T10:00:00Z",
                ),
            ])
        }
    }

    #[test]
    fn apps_show_expiry_and_free_slots() {
        // 2026-11-08T12:00:00Z
        let now = 1_794_139_200;
        let data = apps_data(&Apps, None, now, true).unwrap();
        let list = data["apps"].as_array().unwrap();
        assert_eq!(list[0]["original_id"], "org.forgemobile.gooddog");
        assert_eq!(list[0]["free_team"], true);
        assert_eq!(list[0]["expired"], false);
        assert!((list[0]["days_left"].as_f64().unwrap() - 6.9166).abs() < 0.01);
        assert_eq!(list[1]["expired"], true);
        assert!(list[2]["team"].is_null());
        // The expired app doesn't hold a slot; the App Store app never does.
        assert_eq!(data["free_team_slots"]["in_use"], 1);
        let shown = text(|out| apps(&Apps, None, now, false, out));
        assert!(shown.contains("expires in 6.9 days"), "{shown}");
        // App Store apps (no profile) are hidden by default, and counted.
        assert!(!shown.contains("com.example.store"), "{shown}");
        assert!(
            shown.contains("1 app(s) with no provisioning profile"),
            "{shown}"
        );
        let default = apps_data(&Apps, None, now, false).unwrap();
        assert_eq!(default["apps"].as_array().unwrap().len(), 2);
        assert_eq!(default["not_shown_without_profile"], 1);
        assert_eq!(default["free_team_slots"]["in_use"], 1);
        assert!(shown.contains("profile expired"), "{shown}");
        assert!(shown.contains("Free-team slots: 1 of 3 in use"), "{shown}");
    }

    fn text(f: impl FnOnce(&mut Vec<u8>) -> std::result::Result<(), String>) -> String {
        let mut out = Vec::new();
        f(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn info_explains_developer_mode() {
        let shown = text(|out| info(&Fake, None, out));
        assert!(shown.contains("Name:           Test iPhone"));
        assert!(shown.contains("iOS:            18.1 (22B83)"));
        assert!(shown.contains("Developer Mode: off (Settings > Privacy & Security"));
        assert!(text(|out| list(&Fake, out)).contains("00008110-TEST  Test iPhone"));
    }

    #[test]
    fn logs_keep_one_app_and_stop_at_the_limit() {
        let filter = LogFilter {
            process: Some("Chat".into()),
            contains: None,
        };
        let shown = text(|out| logs(&Fake, None, &filter, Some(2), None, out));
        assert_eq!(
            shown,
            "Nov  7 12:00 iPhone Chat[1] <Notice>: [chat] opened\n\
             Nov  7 12:01 iPhone Chat[1] <Notice>: [chat] sent\n"
        );
    }

    fn crash_fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../forge-crash/tests/fixtures")
    }

    fn query<'a>(app: &'a str, pull: Option<&'a Path>) -> CrashQuery<'a> {
        CrashQuery {
            app: Some(app),
            pull,
            ..CrashQuery::default()
        }
    }

    #[test]
    fn crashes_list_and_pull_one_apps_reports() {
        assert_eq!(
            text(|out| crashes(&Fake, &query("Chat", None), out)),
            "Chat-2026-11-07-120000.ips\n"
        );
        assert_eq!(
            text(|out| crashes(&Fake, &query("Notes", None), out)),
            "No crash reports for Notes on the phone.\n"
        );
        let dir = tempfile::tempdir().unwrap();
        let shown = text(|out| crashes(&Fake, &query("Chat", Some(dir.path())), out));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("Chat-2026-11-07-120000.ips")).unwrap(),
            "report Chat-2026-11-07-120000.ips"
        );
        // Not a real report: copied, but said so rather than failing.
        assert!(
            shown.contains("  not symbolicated: not an .ips crash report"),
            "{shown}"
        );
    }

    #[test]
    fn pulled_crashes_are_symbolicated_against_the_app_build() {
        // An app folder whose build has the crashed binary's dSYM.
        let app = tempfile::tempdir().unwrap();
        let dwarf = app
            .path()
            .join(".build/debug/Crashy.dSYM/Contents/Resources/DWARF");
        std::fs::create_dir_all(&dwarf).unwrap();
        std::fs::copy(
            crash_fixtures().join("Crashy.dSYM/Contents/Resources/DWARF/Crashy"),
            dwarf.join("Crashy"),
        )
        .unwrap();
        let pulled = tempfile::tempdir().unwrap();
        let q = CrashQuery {
            project: Some(app.path()),
            limit: Some(1),
            ..query("Crashy", Some(pulled.path()))
        };
        let shown = text(|out| crashes(&Fake, &q, out));
        assert!(
            shown.contains("  Crashy crashed: trap (EXC_BREAKPOINT, SIGTRAP)"),
            "{shown}"
        );
        assert!(shown.contains("  Likely cause: trap_here at "), "{shown}");
        assert!(shown.contains("crashy.c:5:9"), "{shown}");
        let txt = pulled.path().join("Crashy-2026-11-08-101500.ips.txt");
        assert!(std::fs::read_to_string(txt)
            .unwrap()
            .contains("Thread 0 (crashed"));

        let data = crashes_data(&Fake, &q).unwrap();
        let c = &data["crashes"][0];
        assert_eq!(c["kind"], "trap");
        assert_eq!(c["build"]["status"], "matched");
        assert_eq!(c["culprit"]["function"], "trap_here");
        assert_eq!(c["culprit"]["line"], 5);
        assert_eq!(c["thread"]["frames"][1]["function"], "depth");
        assert!(c["file"]
            .as_str()
            .unwrap()
            .ends_with("Crashy-2026-11-08-101500.ips"));
    }

    #[test]
    fn one_unreadable_report_does_not_stop_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let q = CrashQuery {
            pull: Some(dir.path()),
            ..CrashQuery::default()
        };
        let shown = text(|out| crashes(&Fake, &q, out));
        assert!(
            shown.contains("not copied: Mail-2026-11-07-120000.ips: "),
            "{shown}"
        );
        assert!(dir.path().join("Chat-2026-11-07-120000.ips").is_file());
        let data = crashes_data(&Fake, &q).unwrap();
        assert_eq!(data["skipped"][0]["name"], "Mail-2026-11-07-120000.ips");
        assert_eq!(data["pulled"].as_array().unwrap().len(), 3);
        // The resource report is listed apart, not as a failed crash.
        assert_eq!(data["not_crashes"][0]["bug_type"], "145");
        assert_eq!(data["crashes"].as_array().unwrap().len(), 2);
        assert!(
            shown.contains("  not a crash report (bug type 145): skipped"),
            "{shown}"
        );
    }

    #[test]
    fn a_wrong_udid_is_an_error() {
        let mut out = Vec::new();
        let e = info(&Fake, Some("nope"), &mut out).unwrap_err();
        assert!(e.contains("no connected device has UDID nope"), "{e}");
    }
}
