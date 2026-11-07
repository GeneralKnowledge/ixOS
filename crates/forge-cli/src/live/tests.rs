//! The CLI against a real development server over loopback TCP, through a
//! fake backend that stands in for usbmuxd.

use super::*;
use std::net::TcpStream;
use std::time::Instant;

use forge_device::{Connection, DeviceInfo, LiveStream, Result};
use forge_devtools::{Config, DevTools};
use forge_runtime::{logging, Lifecycle, Phase};

struct Phone {
    tools: DevTools,
}

fn phone() -> DeviceRef {
    DeviceRef {
        udid: "00008130-TEST".into(),
        connection: Connection::Usb,
    }
}

impl DeviceBackend for Phone {
    fn devices(&self) -> Result<Vec<DeviceRef>> {
        Ok(vec![phone()])
    }
    fn info(&self, _: &DeviceRef) -> Result<DeviceInfo> {
        unreachable!()
    }
    fn syslog(&self, _: &DeviceRef, _: &mut dyn FnMut(&str) -> bool) -> Result<()> {
        unreachable!()
    }
    fn crash_reports(&self, _: &DeviceRef) -> Result<Vec<String>> {
        unreachable!()
    }
    fn pull_crash_report(&self, _: &DeviceRef, _: &str) -> Result<Vec<u8>> {
        unreachable!()
    }
    fn connect_port(&self, _: &DeviceRef, _: u16) -> Result<Box<dyn LiveStream>> {
        // Like usbmuxd: refused while the app is not listening.
        let port = self.tools.port().ok_or(DeviceError::AppNotListening)?;
        let stream =
            TcpStream::connect(("127.0.0.1", port)).map_err(|_| DeviceError::AppNotListening)?;
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        Ok(Box::new(stream))
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    store: PairingStore,
    lifecycle: Lifecycle,
    phone: Phone,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let lifecycle = Lifecycle::new();
    let mut config = Config::new("org.example.cli", "0.1.0", dir.path().join("app/pairing"));
    config.port = 0;
    let tools = DevTools::start(config, lifecycle.clone()).unwrap();
    tools.probe("notes", || "2".into());
    tools.command("greet", "Says hello", |args| {
        Ok(format!("hello {}", args.join(" ")))
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while tools.port().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    Rig {
        store: PairingStore::at(dir.path().join("host")),
        _dir: dir,
        lifecycle,
        phone: Phone { tools },
    }
}

impl Rig {
    fn target(&self) -> Target<'_> {
        Target {
            backend: &self.phone,
            udid: None,
            port: 0,
            store: &self.store,
        }
    }

    fn run(
        &self,
        f: impl FnOnce(&Target, &mut dyn Write) -> std::result::Result<(), String>,
    ) -> String {
        let mut out = Vec::new();
        f(&self.target(), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }
}

#[test]
fn pair_then_status_snapshot_and_commands() {
    let rig = rig();
    let e = status(&rig.target(), &mut Vec::new()).unwrap_err();
    assert!(e.contains("not paired with this computer"), "{e}");

    let paired = rig.run(pair);
    assert!(
        paired.starts_with("Paired with org.example.cli on 00008130-TEST."),
        "{paired}"
    );
    assert!(rig.store.load("00008130-TEST", "org.example.cli").is_some());

    let shown = rig.run(status);
    assert!(
        shown.contains("App:      org.example.cli (build 0.1.0)"),
        "{shown}"
    );
    assert!(shown.contains("Phase:    Launching"), "{shown}");
    assert!(shown.contains("Probes:   notes"), "{shown}");
    assert!(shown.contains("Commands: greet"), "{shown}");

    assert_eq!(rig.run(snapshot), "notes  2\n");
    assert_eq!(
        rig.run(|t, out| cmd(t, None, &[], out)),
        "greet  Says hello\n"
    );
    assert_eq!(
        rig.run(|t, out| cmd(t, Some("greet"), &["Reuben".into()], out)),
        "hello Reuben\n"
    );
    let e = cmd(&rig.target(), Some("nope"), &[], &mut Vec::new()).unwrap_err();
    assert!(e.contains("no command named"), "{e}");

    assert_eq!(
        rig.run(unpair),
        "Unpaired org.example.cli on 00008130-TEST.\n"
    );
    assert!(rig.store.load("00008130-TEST", "org.example.cli").is_none());
}

#[test]
fn connect_streams_logs_and_stops_at_the_background() {
    let rig = rig();
    rig.run(pair);
    rig.lifecycle.enter(Phase::Active);
    logging::info!(target: "cli-test", "first line");
    let shown = rig.run(|t, out| {
        connect(t, 50, None, false, {
            // Send the app to the background once the stream is running.
            let lifecycle = rig.lifecycle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                logging::info!(target: "cli-test", "last line");
                lifecycle.enter(Phase::Background);
            });
            out
        })
    });
    assert!(
        shown.starts_with("Connected to org.example.cli (build 0.1.0), Active."),
        "{shown}"
    );
    assert!(shown.contains("INFO  [cli-test] first line"), "{shown}");
    assert!(shown.contains("INFO  [cli-test] last line"), "{shown}");
    assert!(shown.contains("--- phase: Background"), "{shown}");
    assert!(
        shown.ends_with("[suspended: the app is in the background]\n"),
        "{shown}"
    );
}

#[test]
fn connect_follows_the_app_back_from_the_background() {
    let rig = rig();
    rig.run(pair);
    rig.lifecycle.enter(Phase::Active);
    let lifecycle = rig.lifecycle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        lifecycle.enter(Phase::Background);
        // After the app has said goodbye, so it can only arrive by replay.
        std::thread::sleep(Duration::from_millis(600));
        logging::info!(target: "cli-follow", "logged while away");
        std::thread::sleep(Duration::from_millis(1500));
        lifecycle.enter(Phase::Active);
        logging::info!(target: "cli-follow", "back again");
    });
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    // Stop once "back again" is printed: count lines as they come.
    struct Until<'a>(&'a mut Vec<u8>);
    impl Write for Until<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            if String::from_utf8_lossy(self.0).contains("back again") {
                return Err(std::io::Error::other("enough"));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let _ = connect(&rig.target(), 0, None, true, &mut Until(&mut out));
    assert!(Instant::now() < deadline);
    let shown = String::from_utf8(out).unwrap();
    assert!(
        shown.contains("[suspended: the app is in the background; waiting"),
        "{shown}"
    );
    assert!(
        shown.contains("[back: org.example.cli is Active]"),
        "{shown}"
    );
    let away = shown.find("logged while away").expect(&shown);
    let back = shown.find("[back:").unwrap();
    assert!(
        away > back,
        "the record logged while away arrives after reconnecting: {shown}"
    );
    assert!(shown.contains("back again"), "{shown}");
}
