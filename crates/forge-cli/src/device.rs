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
        data["crashes"] = json!(analyses
            .iter()
            .map(|(f, a)| crate::crash::entry(f, a))
            .collect::<Vec<_>>());
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
            Ok(format!("report {name}").into_bytes())
        }
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
        assert_eq!(data["pulled"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_wrong_udid_is_an_error() {
        let mut out = Vec::new();
        let e = info(&Fake, Some("nope"), &mut out).unwrap_err();
        assert!(e.contains("no connected device has UDID nope"), "{e}");
    }
}
