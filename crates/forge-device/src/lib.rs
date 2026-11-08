//! Developer access to a connected iPhone from Linux (Forge Device M0).
//!
//! Everything here talks to the phone's lockdown services through
//! `usbmuxd` (the daemon that ships with every Linux distribution's
//! `usbmuxd` package), using the pairing record the phone created when it
//! was trusted. Nothing needs root, a Mac or the iOS 17 developer tunnel.
//!
//! - [`DeviceBackend`]: what the CLI needs from a phone. [`UsbBackend`]
//!   implements it with the `idevice` crate; tests use a fake.
//! - [`pick_device`]: choose the phone to talk to.
//! - [`LogFilter`]: keep only one app's lines from the system log.
//! - [`app_crash_reports`]: crash reports written for one app.
//! - [`live`]: the live connection to a development build (M1): pairing,
//!   status, probes, commands and the app's own log stream.
//!
//! The phone side needs nothing from the app: logs come from the system log
//! (where `forge_runtime::logging` writes), crash reports from the phone's
//! crash reporter.

pub mod live;
mod usb;

pub use live::{LiveStream, PairingStore};
pub use usb::UsbBackend;

/// How a phone is connected to this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
    Usb,
    /// Wi-Fi sync, with the phone's address.
    Network(String),
    Other(String),
}

impl std::fmt::Display for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Connection::Usb => write!(f, "USB"),
            Connection::Network(addr) => write!(f, "network ({addr})"),
            Connection::Other(kind) => write!(f, "{kind}"),
        }
    }
}

/// A phone usbmuxd can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRef {
    pub udid: String,
    pub connection: Connection,
}

/// What lockdown says about a phone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub udid: String,
    pub connection: Connection,
    /// The name the owner gave it ("Reuben's iPhone").
    pub name: String,
    /// Hardware model, for example `iPhone15,2`.
    pub product_type: String,
    /// iOS version, for example `18.1`.
    pub ios_version: String,
    /// iOS build, for example `22B83`.
    pub build_version: String,
    /// Whether Developer Mode is on (iOS 16+). `None` when the phone does
    /// not report it.
    pub developer_mode: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error(
        "cannot reach usbmuxd ({reason}). Plug the iPhone in and unlock it: on Linux \
         usbmuxd only runs while a phone is connected. If it is plugged in, install \
         usbmuxd: `sudo apt install usbmuxd`"
    )]
    Usbmuxd { reason: String },
    #[error("no iPhone connected. Plug it in with a cable, unlock it and tap Trust if asked")]
    NoDevice,
    #[error("several devices are connected ({}); pick one with --udid", .0.join(", "))]
    SeveralDevices(Vec<String>),
    #[error("no connected device has UDID {0}")]
    NotFound(String),
    #[error(
        "this computer is not trusted by the phone ({reason}). Unlock the phone, \
         replug the cable and tap Trust"
    )]
    NotPaired { reason: String },
    #[error("the phone refused or dropped the request: {0}")]
    Service(String),
    #[error(
        "no development build is listening on the phone. Open the app (it must be on \
         screen) and check it was installed with `forge run ios --devtools`"
    )]
    AppNotListening,
    #[error(transparent)]
    Live(#[from] forge_devproto::client::ClientError),
    #[error("{0}")]
    Pairings(String),
}

pub type Result<T> = std::result::Result<T, DeviceError>;

/// What `forge device` needs from a phone.
pub trait DeviceBackend {
    /// Phones usbmuxd can see right now.
    fn devices(&self) -> Result<Vec<DeviceRef>>;

    /// Name, model, iOS version and Developer Mode of one phone.
    fn info(&self, device: &DeviceRef) -> Result<DeviceInfo>;

    /// Streams the phone's system log, one line at a time, until `on_line`
    /// returns `false` or the connection ends.
    fn syslog(&self, device: &DeviceRef, on_line: &mut dyn FnMut(&str) -> bool) -> Result<()>;

    /// Like [`syslog`](Self::syslog), but also stops at `until`, even when
    /// the phone logs nothing (`forge device logs --for`). The default only
    /// checks the time when a line arrives; backends that can wait with a
    /// timeout override it.
    fn syslog_until(
        &self,
        device: &DeviceRef,
        until: Option<std::time::Instant>,
        on_line: &mut dyn FnMut(&str) -> bool,
    ) -> Result<()> {
        match until {
            None => self.syslog(device, on_line),
            Some(until) => self.syslog(device, &mut |line| {
                std::time::Instant::now() < until && on_line(line)
            }),
        }
    }

    /// Like [`syslog`](Self::syslog), but also stops soon after `stop` is
    /// set, even when the phone logs nothing (the watch during a UI run,
    /// which must not outlive the run). The default only checks `stop` when
    /// a line arrives; backends that can wait with a timeout override it.
    fn syslog_while(
        &self,
        device: &DeviceRef,
        stop: &std::sync::atomic::AtomicBool,
        on_line: &mut dyn FnMut(&str) -> bool,
    ) -> Result<()> {
        self.syslog(device, &mut |line| {
            !stop.load(std::sync::atomic::Ordering::Relaxed) && on_line(line)
        })
    }

    /// Names of the crash reports on the phone (paths relative to the crash
    /// report folder).
    fn crash_reports(&self, device: &DeviceRef) -> Result<Vec<String>>;

    /// One crash report's contents.
    fn pull_crash_report(&self, device: &DeviceRef, name: &str) -> Result<Vec<u8>>;

    /// Apps the developer installed (not Apple's), with their names and
    /// versions (`forge device apps`).
    fn user_apps(&self, device: &DeviceRef) -> Result<Vec<InstalledApp>> {
        let _ = device;
        Err(DeviceError::Service("this backend cannot list apps".into()))
    }

    /// The provisioning profiles installed on the phone, as the plist each
    /// one carries (signed CMS, unparsed: see [`profile_from_der`]).
    fn provisioning_profiles(&self, device: &DeviceRef) -> Result<Vec<Vec<u8>>> {
        let _ = device;
        Err(DeviceError::Service(
            "this backend cannot list provisioning profiles".into(),
        ))
    }

    /// A byte stream to `port` on the phone's loopback interface, where a
    /// development build listens. Reads give up after a short wait (an
    /// `ErrorKind::TimedOut` error) so a session loop can do other work.
    fn connect_port(&self, device: &DeviceRef, port: u16) -> Result<Box<dyn LiveStream>> {
        let _ = (device, port);
        Err(DeviceError::Service(
            "this backend cannot open connections".into(),
        ))
    }
}

/// An app the developer installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledApp {
    /// As installed: xtool's free-team installs are `XTL-<team>.<id>`.
    pub bundle_id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    /// The executable (`CFBundleExecutable`): crash reports and log lines
    /// use it.
    pub executable: Option<String>,
}

/// What a provisioning profile allows, read from its embedded plist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub team: Option<String>,
    /// `application-identifier`: `<team>.<bundle id>`, possibly ending `*`.
    pub app_id: Option<String>,
    /// Seconds since the Unix epoch.
    pub created: Option<i64>,
    pub expires: Option<i64>,
}

impl Profile {
    /// The bundle id the profile signs, without the team prefix.
    pub fn bundle_id(&self) -> Option<&str> {
        self.app_id.as_deref()?.split_once('.').map(|(_, id)| id)
    }

    /// A free team's profiles last 7 days; a paid team's up to a year.
    pub fn lifetime_days(&self) -> Option<i64> {
        Some((self.expires? - self.created?) / 86_400)
    }
}

/// Reads the plist inside a signed provisioning profile (a CMS envelope with
/// the XML plist as its content; the signature isn't checked: the phone
/// already did). `None` when there's no plist to read.
pub fn profile_from_der(der: &[u8]) -> Option<Profile> {
    let start = find(der, b"<?xml")?;
    let end = find(&der[start..], b"</plist>")? + start + b"</plist>".len();
    let dict = plist::Value::from_reader_xml(&der[start..end])
        .ok()?
        .into_dictionary()?;
    let text = |key: &str| dict.get(key).and_then(|v| v.as_string()).map(str::to_owned);
    let date = |key: &str| {
        dict.get(key)
            .and_then(|v| v.as_date())
            .and_then(|d| {
                std::time::SystemTime::from(d)
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
            })
            .map(|d| d.as_secs() as i64)
    };
    let app_id = dict
        .get("Entitlements")
        .and_then(|e| e.as_dictionary())
        .and_then(|e| e.get("application-identifier"))
        .and_then(|v| v.as_string())
        .map(str::to_owned);
    let team = dict
        .get("TeamIdentifier")
        .and_then(|t| t.as_array())
        .and_then(|t| t.first())
        .and_then(|v| v.as_string())
        .map(str::to_owned);
    Some(Profile {
        name: text("Name").unwrap_or_default(),
        team,
        app_id,
        created: date("CreationDate"),
        expires: date("ExpirationDate"),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The phone to use: the one with `udid`, or the only one connected.
pub fn pick_device(backend: &dyn DeviceBackend, udid: Option<&str>) -> Result<DeviceRef> {
    let devices = backend.devices()?;
    match udid {
        Some(udid) => devices
            .into_iter()
            .find(|d| d.udid == udid)
            .ok_or_else(|| DeviceError::NotFound(udid.to_string())),
        None => {
            // A phone on USB and Wi-Fi shows up twice: prefer USB.
            let mut unique: Vec<DeviceRef> = Vec::new();
            for device in devices {
                match unique.iter_mut().find(|d| d.udid == device.udid) {
                    Some(existing) if device.connection == Connection::Usb => *existing = device,
                    Some(_) => {}
                    None => unique.push(device),
                }
            }
            match unique.len() {
                0 => Err(DeviceError::NoDevice),
                1 => Ok(unique.remove(0)),
                _ => Err(DeviceError::SeveralDevices(
                    unique.into_iter().map(|d| d.udid).collect(),
                )),
            }
        }
    }
}

/// Which system log lines to keep.
#[derive(Clone, Debug, Default)]
pub struct LogFilter {
    /// Only lines from this process (the app's executable name, for
    /// example `Chat`).
    pub process: Option<String>,
    /// Only lines containing this text (case-insensitive).
    pub contains: Option<String>,
}

impl LogFilter {
    pub fn matches(&self, line: &str) -> bool {
        if let Some(process) = &self.process {
            if log_process(line) != Some(process.as_str()) {
                return false;
            }
        }
        if let Some(text) = &self.contains {
            if !line.to_lowercase().contains(&text.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

/// The process name of a syslog relay line:
/// `Nov  7 12:00:01 iPhone Chat(libforge_ffi.dylib)[512] <Notice>: ...` gives
/// `Chat`. `None` if the line does not have that shape.
pub fn log_process(line: &str) -> Option<&str> {
    // The process field is the first one followed by `[pid]` before the
    // `<Level>:` marker.
    let head = &line[..line.find(" <")?];
    let field = head.rsplit(' ').next()?;
    let name = field.split(['[', '(']).next()?;
    (!name.is_empty() && field.contains('[')).then_some(name)
}

/// Crash reports written for `app` (its executable name), newest first.
/// iOS names them `<App>-<date>-<time>.ips`; older ones end in `.crash`.
pub fn app_crash_reports(names: &[String], app: &str) -> Vec<String> {
    let prefix = format!("{app}-");
    let mut matching: Vec<String> = names
        .iter()
        .filter(|name| {
            let file = name.rsplit('/').next().unwrap_or(name);
            file.starts_with(&prefix) && (file.ends_with(".ips") || file.ends_with(".crash"))
        })
        .cloned()
        .collect();
    // The date in the file name sorts lexically; ignore the folder.
    let file = |name: &String| name.rsplit('/').next().unwrap_or(name).to_string();
    matching.sort_by_key(|name| std::cmp::Reverse(file(name)));
    matching
}

#[cfg(test)]
mod tests {

    #[test]
    fn reads_the_plist_inside_a_signed_profile() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Name</key><string>XC org forgemobile gooddog</string>
<key>TeamIdentifier</key><array><string>ND4P87PX52</string></array>
<key>CreationDate</key><date>2026-11-08T10:00:00Z</date>
<key>ExpirationDate</key><date>2026-11-15T10:00:00Z</date>
<key>Entitlements</key><dict><key>application-identifier</key><string>ND4P87PX52.XTL-ND4P87PX52.org.forgemobile.gooddog</string></dict>
</dict></plist>"#;
        let mut der = vec![0x30, 0x82, 0x10, 0x00, 0x06, 0x09];
        der.extend_from_slice(xml.as_bytes());
        der.extend_from_slice(&[0xa0, 0x82, 0x01]);
        let p = profile_from_der(&der).unwrap();
        assert_eq!(p.team.as_deref(), Some("ND4P87PX52"));
        assert_eq!(
            p.bundle_id(),
            Some("XTL-ND4P87PX52.org.forgemobile.gooddog")
        );
        assert_eq!(p.lifetime_days(), Some(7));
        assert_eq!(p.expires, Some(1_794_736_800));
        assert!(profile_from_der(b"no plist here").is_none());
    }
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        devices: Vec<DeviceRef>,
        lines: Vec<&'static str>,
        crashes: Vec<String>,
        pulled: RefCell<Vec<String>>,
    }

    impl DeviceBackend for Fake {
        fn devices(&self) -> Result<Vec<DeviceRef>> {
            Ok(self.devices.clone())
        }
        fn info(&self, device: &DeviceRef) -> Result<DeviceInfo> {
            Ok(DeviceInfo {
                udid: device.udid.clone(),
                connection: device.connection.clone(),
                name: "Test iPhone".into(),
                product_type: "iPhone15,2".into(),
                ios_version: "18.1".into(),
                build_version: "22B83".into(),
                developer_mode: Some(true),
            })
        }
        fn syslog(&self, _: &DeviceRef, on_line: &mut dyn FnMut(&str) -> bool) -> Result<()> {
            for line in &self.lines {
                if !on_line(line) {
                    break;
                }
            }
            Ok(())
        }
        fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
            Ok(self.crashes.clone())
        }
        fn pull_crash_report(&self, _: &DeviceRef, name: &str) -> Result<Vec<u8>> {
            self.pulled.borrow_mut().push(name.to_string());
            Ok(name.as_bytes().to_vec())
        }
    }

    fn usb(udid: &str) -> DeviceRef {
        DeviceRef {
            udid: udid.into(),
            connection: Connection::Usb,
        }
    }

    fn fake(devices: Vec<DeviceRef>) -> Fake {
        Fake {
            devices,
            lines: vec![],
            crashes: vec![],
            pulled: RefCell::new(vec![]),
        }
    }

    #[test]
    fn picks_the_only_phone_or_asks_for_a_udid() {
        assert!(matches!(
            pick_device(&fake(vec![]), None),
            Err(DeviceError::NoDevice)
        ));
        assert_eq!(pick_device(&fake(vec![usb("a")]), None).unwrap(), usb("a"));
        assert!(matches!(
            pick_device(&fake(vec![usb("a"), usb("b")]), None),
            Err(DeviceError::SeveralDevices(ids)) if ids == ["a", "b"]
        ));
        assert_eq!(
            pick_device(&fake(vec![usb("a"), usb("b")]), Some("b")).unwrap(),
            usb("b")
        );
        assert!(matches!(
            pick_device(&fake(vec![usb("a")]), Some("z")),
            Err(DeviceError::NotFound(id)) if id == "z"
        ));
    }

    #[test]
    fn a_phone_on_usb_and_wifi_counts_once_and_prefers_usb() {
        let wifi = DeviceRef {
            udid: "a".into(),
            connection: Connection::Network("192.168.1.5".into()),
        };
        let backend = fake(vec![wifi, usb("a")]);
        assert_eq!(pick_device(&backend, None).unwrap(), usb("a"));
    }

    #[test]
    fn finds_the_process_of_a_syslog_line() {
        assert_eq!(
            log_process("Nov  7 12:00:01 iPhone Chat(libforge_ffi.dylib)[512] <Notice>: [chat] hi"),
            Some("Chat")
        );
        assert_eq!(
            log_process("Nov  7 12:00:01 Reubens-iPhone SpringBoard[60] <Error>: x"),
            Some("SpringBoard")
        );
        assert_eq!(log_process("garbage"), None);
        assert_eq!(
            log_process("Nov  7 12:00:01 iPhone kernel <Notice>: x"),
            None
        );
    }

    #[test]
    fn filters_log_lines_by_process_and_text() {
        let filter = LogFilter {
            process: Some("Chat".into()),
            contains: Some("SENT".into()),
        };
        assert!(filter.matches("Nov  7 12:00 iPhone Chat[1] <Notice>: [chat] message sent"));
        assert!(!filter.matches("Nov  7 12:00 iPhone Chat[1] <Notice>: [chat] opened"));
        assert!(!filter.matches("Nov  7 12:00 iPhone Mail[2] <Notice>: message sent"));
        assert!(LogFilter::default().matches("anything"));
    }

    #[test]
    fn syslog_until_a_past_deadline_shows_nothing() {
        let mut backend = fake(vec![usb("a")]);
        backend.lines = vec!["one", "two"];
        let mut seen = 0;
        backend
            .syslog_until(&usb("a"), Some(std::time::Instant::now()), &mut |_| {
                seen += 1;
                true
            })
            .unwrap();
        assert_eq!(seen, 0);
        backend
            .syslog_until(&usb("a"), None, &mut |_| {
                seen += 1;
                true
            })
            .unwrap();
        assert_eq!(seen, 2);
    }

    #[test]
    fn syslog_stops_when_asked() {
        let mut backend = fake(vec![usb("a")]);
        backend.lines = vec!["one", "two", "three"];
        let mut seen = vec![];
        backend
            .syslog(&usb("a"), &mut |line| {
                seen.push(line.to_string());
                seen.len() < 2
            })
            .unwrap();
        assert_eq!(seen, ["one", "two"]);
    }

    #[test]
    fn lists_one_apps_crash_reports_newest_first() {
        let names: Vec<String> = [
            "Chat-2026-11-07-101500.ips",
            "Chat-2026-11-07-120000.ips",
            "ChatHelper-2026-11-07-120000.ips",
            "Retired/Chat-2025-01-01-000000.crash",
            "Chat-notes.txt",
            "JetsamEvent-2026-11-07.ips",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            app_crash_reports(&names, "Chat"),
            [
                "Chat-2026-11-07-120000.ips",
                "Chat-2026-11-07-101500.ips",
                "Retired/Chat-2025-01-01-000000.crash",
            ]
        );
    }
}
