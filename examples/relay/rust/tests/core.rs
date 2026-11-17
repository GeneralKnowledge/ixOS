//! Two Relay devices on one machine, talking over localhost, as the app
//! and the `relay` command will.

use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{Direction, Item, RelayCore, RelayError, TransferStatus};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn device(dir: &tempfile::TempDir, name: &str) -> Arc<RelayCore> {
    RelayCore::new(
        dir.path().join(name).to_string_lossy().into_owned(),
        name.into(),
    )
    .unwrap()
}

/// Waits for the listening side to record what arrived.
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A listening phone and a laptop that paired with it.
fn paired(dir: &tempfile::TempDir) -> (Arc<RelayCore>, Arc<RelayCore>) {
    let phone = device(dir, "phone");
    let laptop = device(dir, "laptop");
    phone.start_listening(0).unwrap();
    let invitation = phone.create_invitation(vec!["127.0.0.1".into()]).unwrap();
    let peer = block_on(laptop.pair(invitation.code, None)).unwrap();
    assert_eq!(peer.name, "phone");
    eventually("the phone to keep the laptop", || phone.peers().len() == 1);
    (phone, laptop)
}

#[test]
fn pairing_by_code_lets_both_sides_send() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop) = paired(&dir);
    let phone_id = phone.identity().device_id;
    assert_eq!(phone.peers()[0].name, "laptop");
    let port = phone.listening_port().unwrap();

    let sent = block_on(laptop.send_note(
        phone_id.clone(),
        "  Remember the milk ".into(),
        vec![format!("127.0.0.1:{port}")],
        None,
    ))
    .unwrap();
    assert_eq!(sent.status, TransferStatus::Done);
    assert_eq!(sent.direction, Direction::Sent);
    eventually("the note", || {
        phone.transfers().first().map(|t| &t.status) == Some(&TransferStatus::Done)
    });
    let got = &phone.transfers()[0];
    assert_eq!(got.direction, Direction::Received);
    assert_eq!(got.peer_name, "laptop");
    assert_eq!(
        got.item,
        Item::Note {
            text: "Remember the milk".into()
        }
    );

    // A file, bigger than one chunk; the laptop remembers where the phone
    // was, so no address is needed this time.
    let source = dir.path().join("Harbour photo.jpg");
    let bytes: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &bytes).unwrap();
    let sent = block_on(laptop.send_file(
        phone_id,
        source.to_string_lossy().into_owned(),
        vec![],
        None,
    ))
    .unwrap();
    assert_eq!(sent.status, TransferStatus::Done);
    eventually("the file", || {
        phone.transfers().len() == 2 && phone.transfers()[0].status == TransferStatus::Done
    });
    let Item::File { name, size, path } = phone.transfers()[0].item.clone() else {
        panic!("a file")
    };
    assert_eq!((name.as_str(), size), ("Harbour photo.jpg", 100_000));
    let path = path.unwrap();
    assert!(path.ends_with("Harbour-photo.jpg"), "{path}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    // Removing the entry removes the received file.
    let id = phone.transfers()[0].id;
    block_on(phone.remove_transfer(id)).unwrap();
    assert!(!std::path::Path::new(&path).exists());
}

#[test]
fn a_code_works_once_and_strangers_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let phone = device(&dir, "phone");
    let laptop = device(&dir, "laptop");
    let other = device(&dir, "other");
    assert!(
        phone.create_invitation(vec!["127.0.0.1".into()]).is_err(),
        "not listening"
    );
    phone.start_listening(0).unwrap();
    let code = phone
        .create_invitation(vec!["127.0.0.1".into()])
        .unwrap()
        .code;
    block_on(laptop.pair(code.clone(), None)).unwrap();
    assert_eq!(
        block_on(other.pair(code, None)),
        Err(RelayError::Refused),
        "the code was used"
    );

    // A code from another phone doesn't open this one.
    let fake = phone
        .create_invitation(vec!["127.0.0.1".into()])
        .unwrap()
        .code;
    phone.cancel_invitation();
    assert_eq!(block_on(other.pair(fake, None)), Err(RelayError::Refused));
    assert_eq!(phone.peers().len(), 1);

    // Unpaired, the laptop can't send.
    let phone_id = phone.identity().device_id;
    block_on(phone.forget_peer(laptop.identity().device_id)).unwrap();
    let port = phone.listening_port().unwrap();
    let result = block_on(laptop.send_note(
        phone_id,
        "hello".into(),
        vec![format!("127.0.0.1:{port}")],
        None,
    ));
    assert_eq!(result, Err(RelayError::Refused));
    assert!(matches!(
        laptop.transfers()[0].status,
        TransferStatus::Failed { .. }
    ));
}

#[test]
fn identity_and_peers_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop) = paired(&dir);
    let id = phone.identity();
    block_on(phone.rename_device("Cati".into())).unwrap();
    phone.shutdown();
    // The phone's answer to the pairing connection can still hold the core
    // for a moment after the peer is saved; reopening before it lets go is
    // refused as in use.
    let closing = Arc::downgrade(&phone);
    drop(phone);
    eventually("the old phone core to close", || {
        closing.strong_count() == 0
    });
    let phone = device(&dir, "phone");
    assert_eq!(phone.identity().device_id, id.device_id);
    assert_eq!(phone.identity().name, "Cati");
    assert_eq!(phone.peers()[0].device_id, laptop.identity().device_id);
    assert!(matches!(
        block_on(laptop.send_note(id.device_id, "x".into(), vec!["127.0.0.1:1".into()], None)),
        Err(RelayError::Unreachable { .. })
    ));
}

#[test]
fn bad_input_is_refused_early() {
    let dir = tempfile::tempdir().unwrap();
    let laptop = device(&dir, "laptop");
    assert_eq!(
        block_on(laptop.pair("not a code".into(), None)),
        Err(RelayError::BadCode)
    );
    assert_eq!(
        block_on(laptop.send_note("nobody".into(), "hi".into(), vec![], None)),
        Err(RelayError::UnknownPeer {
            id: "nobody".into()
        })
    );
    assert_eq!(
        block_on(laptop.send_note("nobody".into(), "   ".into(), vec![], None)),
        Err(RelayError::EmptyNote)
    );
}

/// A folder sent with a word code between two devices: it arrives whole,
/// and both histories record it, with the received folder's path.
#[test]
fn a_folder_with_a_code_lands_in_both_histories() {
    use relay_core::CodeSend;
    let dir = tempfile::tempdir().unwrap();
    let phone = device(&dir, "phone");
    let laptop = device(&dir, "laptop");
    let folder = dir.path().join("Trip");
    std::fs::create_dir_all(folder.join("day 1")).unwrap();
    std::fs::write(folder.join("day 1/map.txt"), b"north").unwrap();
    std::fs::write(folder.join("list.txt"), b"tent, stove").unwrap();

    laptop
        .send_with_code(folder.to_string_lossy().into_owned())
        .unwrap();
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = waiting(&laptop)
    else {
        unreachable!()
    };
    let receipt =
        block_on(phone.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id))
            .unwrap();
    let got = std::path::Path::new(&receipt.path);
    assert_eq!(std::fs::read(got.join("day 1/map.txt")).unwrap(), b"north");
    assert_eq!(std::fs::read(got.join("list.txt")).unwrap(), b"tent, stove");

    eventually("the sent entry", || !laptop.transfers().is_empty());
    let sent = &laptop.transfers()[0];
    assert_eq!(sent.direction, Direction::Sent);
    assert_eq!(sent.status, TransferStatus::Done);
    assert_eq!(sent.peer_name, "someone with a code");

    let received = &phone.transfers()[0];
    assert_eq!(received.direction, Direction::Received);
    assert_eq!(received.status, TransferStatus::Done);
    let Item::File { name, size, path } = &received.item else {
        panic!("not a file entry")
    };
    assert_eq!((name.as_str(), *size), ("Trip", 16));
    assert_eq!(path.as_deref(), Some(receipt.path.as_str()));

    // Removing the entry removes the received folder.
    block_on(phone.remove_transfer(received.id)).unwrap();
    assert!(!got.exists());
}

/// The device's endpoint id stays the same across relaunches: sends from
/// the same data directory advertise the same id.
#[test]
fn the_device_keeps_its_endpoint_id() {
    use relay_core::CodeSend;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, b"hello").unwrap();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let laptop = device(&dir, "laptop");
        laptop
            .send_with_code(file.to_string_lossy().into_owned())
            .unwrap();
        let CodeSend::Waiting { endpoint_id, .. } = waiting(&laptop) else {
            unreachable!()
        };
        ids.push(endpoint_id);
        laptop.cancel_code_send();
        eventually("the send to stop", || {
            matches!(laptop.code_send_status(), CodeSend::Failed { .. })
        });
        drop(laptop);
        // The node closes in the background as the core goes.
        std::thread::sleep(Duration::from_millis(500));
    }
    assert_eq!(ids[0], ids[1]);
}

/// The send's status once it has a code.
fn waiting(core: &RelayCore) -> relay_core::CodeSend {
    eventually("a code", || {
        matches!(
            core.code_send_status(),
            relay_core::CodeSend::Waiting { .. }
        )
    });
    core.code_send_status()
}
