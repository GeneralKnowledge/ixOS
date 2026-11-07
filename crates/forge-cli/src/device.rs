//! `forge device`: a connected iPhone's info, live logs and crash reports,
//! from Linux over usbmuxd (Forge Device M0). The phone work is in the
//! `forge-device` crate; this file only formats it.

use std::io::Write;
use std::path::Path;

use forge_device::{app_crash_reports, pick_device, DeviceBackend, LogFilter};

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
/// have matched, or the user presses Ctrl-C.
pub fn logs(
    backend: &dyn DeviceBackend,
    udid: Option<&str>,
    filter: &LogFilter,
    limit: Option<usize>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let device = pick_device(backend, udid).map_err(err)?;
    let mut shown = 0usize;
    let mut write_failed = None;
    backend
        .syslog(&device, &mut |line| {
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
    udid: Option<&str>,
    app: Option<&str>,
    pull: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let device = pick_device(backend, udid).map_err(err)?;
    let all = backend.crash_reports(&device).map_err(err)?;
    let names = match app {
        Some(app) => app_crash_reports(&all, app),
        None => {
            let mut all = all;
            all.sort();
            all
        }
    };
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
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
            for name in &names {
                let bytes = backend.pull_crash_report(&device, name).map_err(err)?;
                let file = dir.join(name.rsplit('/').next().unwrap_or(name));
                std::fs::write(&file, bytes)
                    .map_err(|e| format!("write {}: {e}", file.display()))?;
                writeln!(out, "{}", file.display()).map_err(err)?;
            }
        }
    }
    Ok(())
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
            ])
        }
        fn pull_crash_report(&self, _: &DeviceRef, name: &str) -> Result<Vec<u8>> {
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
        let shown = text(|out| logs(&Fake, None, &filter, Some(2), out));
        assert_eq!(
            shown,
            "Nov  7 12:00 iPhone Chat[1] <Notice>: [chat] opened\n\
             Nov  7 12:01 iPhone Chat[1] <Notice>: [chat] sent\n"
        );
    }

    #[test]
    fn crashes_list_and_pull_one_apps_reports() {
        assert_eq!(
            text(|out| crashes(&Fake, None, Some("Chat"), None, out)),
            "Chat-2026-11-07-120000.ips\n"
        );
        assert_eq!(
            text(|out| crashes(&Fake, None, Some("Notes"), None, out)),
            "No crash reports for Notes on the phone.\n"
        );
        let dir = tempfile::tempdir().unwrap();
        text(|out| crashes(&Fake, None, Some("Chat"), Some(dir.path()), out));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("Chat-2026-11-07-120000.ips")).unwrap(),
            "report Chat-2026-11-07-120000.ips"
        );
    }

    #[test]
    fn a_wrong_udid_is_an_error() {
        let mut out = Vec::new();
        let e = info(&Fake, Some("nope"), &mut out).unwrap_err();
        assert!(e.contains("no connected device has UDID nope"), "{e}");
    }
}
