//! The whole live connection over loopback TCP: the real server against the
//! real host client.

use super::*;
use forge_devproto::client::{self, Client, ClientError, Event, From};
use forge_devproto::{new_secret, Reply};

struct Rig {
    _dir: tempfile::TempDir,
    lifecycle: Lifecycle,
    tools: DevTools,
}

fn rig(window: Duration) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let lifecycle = Lifecycle::new();
    let mut config = Config::new("org.example.test", "1.2.3", dir.path().join("pairing"));
    config.port = 0;
    config.pairing_window = window;
    let tools = DevTools::start(config, lifecycle.clone()).unwrap();
    Rig {
        _dir: dir,
        lifecycle,
        tools,
    }
}

impl Rig {
    /// Connects once the server listens (it starts on its own thread).
    fn dial(&self) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(port) = self.tools.port() {
                if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(50)))
                        .unwrap();
                    return stream;
                }
            }
            assert!(Instant::now() < deadline, "server never listened");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn paired(&self) -> Secret {
        let secret = new_secret();
        let pre = client::pair(self.dial(), &secret).unwrap();
        assert_eq!(pre.app, "org.example.test");
        secret
    }

    fn open(&self, secret: Secret, from: From) -> Result<Client<TcpStream>, ClientError> {
        Client::open(self.dial(), |_| Some(secret), from)
    }
}

/// Reads events until `want` matches one, or fails after 5 s.
fn wait_for(client: &mut Client<TcpStream>, want: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    loop {
        if let Some(event) = client.next_event().unwrap() {
            let done = want(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
        assert!(Instant::now() < deadline, "never saw it; got {seen:?}");
    }
}

fn log_says(event: &Event, text: &str) -> bool {
    matches!(event, Event::Record { record: Record::Log(line), .. } if line.message == text)
}

#[test]
fn pairs_then_streams_logs_and_answers_commands() {
    let rig = rig(PAIRING_WINDOW);
    rig.tools.probe("notes", || "3".into());
    rig.tools
        .command("echo", "Repeats its arguments", |args| Ok(args.join(" ")));
    let secret = rig.paired();

    let mut host = rig.open(secret, From::Tail(10)).unwrap();
    assert_eq!(host.hello.app, "org.example.test");
    assert_eq!(host.hello.build, "1.2.3");

    assert_eq!(host.command(Command::Ping).unwrap(), Reply::Pong);
    let Reply::Status(status) = host.command(Command::Status).unwrap() else {
        panic!("not a status")
    };
    assert_eq!(status.phase, "Launching");
    assert_eq!(status.probes, ["notes"]);
    assert_eq!(status.commands, ["echo"]);
    assert_eq!(
        host.command(Command::Snapshot).unwrap(),
        Reply::Snapshot(vec![Probe {
            name: "notes".into(),
            value: "3".into()
        }])
    );
    assert_eq!(
        host.command(Command::App {
            name: "echo".into(),
            args: vec!["a".into(), "b".into()]
        })
        .unwrap(),
        Reply::Done("a b".into())
    );
    let e = host
        .command(Command::App {
            name: "rm".into(),
            args: vec![],
        })
        .unwrap_err();
    assert!(e.to_string().contains("no command named \"rm\""), "{e}");

    logging::info!(target: "devtools-test", "live line");
    wait_for(&mut host, |e| log_says(e, "live line"));
}

#[test]
fn only_the_paired_secret_opens_a_session() {
    let rig = rig(PAIRING_WINDOW);
    assert!(matches!(
        rig.open(new_secret(), From::Tail(0)),
        Err(ClientError::NotPaired { .. })
    ));
    let secret = rig.paired();
    assert!(matches!(
        rig.open(new_secret(), From::Tail(0)),
        Err(ClientError::KeyRejected { .. })
    ));
    rig.open(secret, From::Tail(0)).unwrap();
}

#[test]
fn a_paired_app_refuses_new_pairings_after_the_window() {
    let rig = rig(Duration::ZERO);
    // Unpaired: pairing is always open.
    let secret = rig.paired();
    let e = client::pair(rig.dial(), &new_secret()).unwrap_err();
    assert!(matches!(e, ClientError::PairingClosed { .. }), "{e}");
    rig.open(secret, From::Tail(0)).unwrap();
}

#[test]
fn a_session_survives_the_background_without_losing_records() {
    let rig = rig(PAIRING_WINDOW);
    let secret = rig.paired();
    let mut host = rig.open(secret, From::Tail(0)).unwrap();
    rig.lifecycle.enter(Phase::Active);
    logging::info!(target: "devtools-test", "before background");
    wait_for(&mut host, |e| log_says(e, "before background"));

    rig.lifecycle.enter(Phase::Background);
    let deadline = Instant::now() + Duration::from_secs(5);
    let bye = loop {
        match host.next_event() {
            Ok(_) => assert!(Instant::now() < deadline, "no goodbye"),
            Err(e) => break e,
        }
    };
    assert!(
        matches!(bye, ClientError::Ended(client::Bye(ByeReason::Background))),
        "{bye}"
    );
    // Not listening while in the background.
    let deadline = Instant::now() + Duration::from_secs(5);
    while rig.tools.port().is_some() {
        assert!(Instant::now() < deadline, "still listening");
        std::thread::sleep(Duration::from_millis(20));
    }

    logging::info!(target: "devtools-test", "while away");
    rig.lifecycle.enter(Phase::Active);
    let mut host = rig.open(secret, From::After(host.last_seq())).unwrap();
    let seen = wait_for(&mut host, |e| log_says(e, "while away"));
    let phases: Vec<_> = seen
        .iter()
        .filter_map(|e| match e {
            Event::Record {
                record: Record::Phase { phase },
                ..
            } => Some(phase.as_str()),
            _ => None,
        })
        .collect();
    // Background was seen before the goodbye; only the return is new.
    assert!(!phases.contains(&"Background"), "{seen:?}");
    assert!(
        !seen.iter().any(|e| log_says(e, "before background")),
        "replayed a record the host already had: {seen:?}"
    );
    // Seen after the reconnect: the record logged while away, then Active.
    wait_for(
        &mut host,
        |e| matches!(e, Event::Record { record: Record::Phase { phase }, .. } if phase == "Active"),
    );
}

#[test]
fn unpair_forgets_the_secret_on_the_app() {
    let rig = rig(Duration::ZERO);
    let secret = rig.paired();
    let mut host = rig.open(secret, From::Tail(0)).unwrap();
    assert_eq!(host.command(Command::Unpair).unwrap(), Reply::Unpaired);
    assert!(!rig.tools.shared.config.pairing_file.exists());
    assert!(matches!(
        rig.open(secret, From::Tail(0)),
        Err(ClientError::NotPaired { .. })
    ));
    // With no pairing left, pairing is open again even outside the window.
    rig.paired();
}

#[test]
fn the_pairing_file_is_private() {
    let rig = rig(PAIRING_WINDOW);
    rig.paired();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&rig.tools.shared.config.pairing_file)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
