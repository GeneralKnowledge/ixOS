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

#[test]
fn identity_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let phone = device(&dir, "phone");
    let id = block_on(phone.device_service()).unwrap().endpoint_id;
    block_on(phone.rename_device("Cati".into())).unwrap();
    phone.shutdown();
    let closing = Arc::downgrade(&phone);
    drop(phone);
    eventually("the old phone core to close", || {
        closing.strong_count() == 0
    });
    let phone = device(&dir, "phone");
    // The same key, so the same device (its store may still be closing).
    eventually("the endpoint to start again", || {
        block_on(phone.device_service()).is_ok()
    });
    assert_eq!(block_on(phone.device_service()).unwrap().endpoint_id, id);
    assert_eq!(phone.identity().name, "Cati");
}

#[test]
fn bad_input_is_refused_early() {
    let dir = tempfile::tempdir().unwrap();
    let laptop = device(&dir, "laptop");
    assert_eq!(
        block_on(laptop.send_note_as_older_build("nobody".into(), "hi".into())),
        Err(RelayError::UnknownPeer {
            id: "nobody".into()
        })
    );
    assert_eq!(
        block_on(laptop.send_note_as_older_build("nobody".into(), "   ".into())),
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

/// Several items go as one transfer, and land in the folder the app chose
/// (its Documents, which the Files app shows).
#[test]
fn several_items_land_in_the_chosen_folder() {
    use relay_core::CodeSend;
    let dir = tempfile::tempdir().unwrap();
    let phone = device(&dir, "phone");
    let laptop = device(&dir, "laptop");
    let documents = dir.path().join("Documents/Received");
    phone
        .receive_into(documents.to_string_lossy().into_owned())
        .unwrap();
    let folder = dir.path().join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("list.txt"), b"tent").unwrap();
    let photo = dir.path().join("photo.jpg");
    std::fs::write(&photo, b"jpeg").unwrap();

    laptop
        .send_paths_with_code(vec![
            folder.to_string_lossy().into_owned(),
            photo.to_string_lossy().into_owned(),
        ])
        .unwrap();
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        name,
        ..
    } = waiting(&laptop)
    else {
        unreachable!()
    };
    assert_eq!(name, "2 items");
    let receipt =
        block_on(phone.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id))
            .unwrap();
    let got = std::path::Path::new(&receipt.path);
    assert!(got.starts_with(&documents), "{got:?}");
    assert_eq!(std::fs::read(got.join("Trip/list.txt")).unwrap(), b"tent");
    assert_eq!(std::fs::read(got.join("photo.jpg")).unwrap(), b"jpeg");

    let received = &phone.transfers()[0];
    let Item::File { path, .. } = &received.item else {
        panic!("not a file entry")
    };
    assert_eq!(path.as_deref(), Some(receipt.path.as_str()));
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
