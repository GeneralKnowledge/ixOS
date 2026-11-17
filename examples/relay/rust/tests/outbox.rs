//! The delivery outbox (Relay M1 stage 3): notes and files for a trusted
//! device that is away wait, survive a relaunch, and arrive once and in
//! order when it is back. Two Relay devices on one machine, over
//! localhost.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{CodeSend, Direction, Item, NearbyDevice, RelayCore, TransferStatus};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

/// Opens a device, waiting while a closing one still holds its folder.
fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let core = loop {
        match RelayCore::new(dir.join(name).to_string_lossy().into_owned(), name.into()) {
            Ok(core) => break core,
            Err(err) => {
                assert!(Instant::now() < deadline, "couldn't open {name}: {err}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    core.receive_into(
        dir.join(name)
            .join("Received")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
    core
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn sees(core: &RelayCore, other: &RelayCore) {
    let service = block_on(other.device_service()).unwrap();
    core.devices_nearby(vec![NearbyDevice {
        endpoint_id: service.endpoint_id,
        addresses: vec![format!("127.0.0.1:{}", service.port)],
    }]);
}

/// A phone and a laptop that made one code transfer and trusted each other.
fn trusted_pair(dir: &Path) -> (Arc<RelayCore>, Arc<RelayCore>, String) {
    let phone = device(dir, "phone");
    let laptop = device(dir, "laptop");
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    laptop
        .send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(laptop.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = laptop.code_send_status()
    else {
        unreachable!()
    };
    block_on(phone.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id))
        .unwrap();
    eventually("the laptop's trust offer", || {
        laptop.trust_offer().is_some()
    });
    let on_phone = phone.trust_offer().unwrap();
    let on_laptop = laptop.trust_offer().unwrap();
    block_on(phone.trust_device(on_phone.endpoint_id)).unwrap();
    block_on(laptop.trust_device(on_laptop.endpoint_id.clone())).unwrap();
    (phone, laptop, on_laptop.endpoint_id)
}

/// What `core` received from trusted devices, oldest first.
fn received(core: &RelayCore) -> Vec<Item> {
    core.transfers()
        .into_iter()
        .rev()
        .filter(|t| t.direction == Direction::Received && t.peer_name == "laptop")
        .filter(|t| t.status == TransferStatus::Done)
        .map(|t| t.item)
        .collect()
}

fn note(text: &str) -> Item {
    Item::Note { text: text.into() }
}

#[test]
fn items_for_a_device_that_is_away_survive_a_relaunch_and_arrive_once_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    // The laptop saw the phone, then the phone went away.
    sees(&laptop, &phone);
    drop(phone);
    std::thread::sleep(Duration::from_millis(500));

    let first = block_on(laptop.send_note_to_device(phone_id.clone(), "one".into())).unwrap();
    assert_eq!(first.status, TransferStatus::Waiting);
    let photo = dir.path().join("photo.jpg");
    std::fs::write(&photo, b"a photo").unwrap();
    let file = block_on(
        laptop.send_to_device(phone_id.clone(), vec![photo.to_string_lossy().into_owned()]),
    )
    .unwrap();
    assert_eq!(file.status, TransferStatus::Waiting);
    // The original can go: the outbox copied it.
    std::fs::remove_file(&photo).unwrap();
    let last = block_on(laptop.send_note_to_device(phone_id.clone(), "two".into())).unwrap();
    assert_eq!(last.status, TransferStatus::Waiting);

    // Relay closes on the laptop and opens again: all three still wait.
    drop(laptop);
    std::thread::sleep(Duration::from_millis(500));
    let laptop = device(dir.path(), "laptop");
    let waiting = laptop
        .transfers()
        .into_iter()
        .filter(|t| t.status == TransferStatus::Waiting)
        .count();
    assert_eq!(waiting, 3);

    // The phone is back, and the laptop sees it.
    let phone = device(dir.path(), "phone");
    sees(&laptop, &phone);
    eventually("everything on the phone", || received(&phone).len() == 3);
    let got = received(&phone);
    assert_eq!(got[0], note("one"));
    let Item::File {
        name,
        path: Some(path),
        ..
    } = &got[1]
    else {
        panic!("not a file: {:?}", got[1])
    };
    assert_eq!(name, "photo.jpg");
    assert_eq!(std::fs::read(path).unwrap(), b"a photo");
    assert_eq!(got[2], note("two"));
    eventually("all done on the laptop", || {
        laptop
            .transfers()
            .iter()
            .filter(|t| t.direction == Direction::Sent)
            .all(|t| t.status == TransferStatus::Done)
    });
    // Nothing more arrives.
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(received(&phone).len(), 3);
}

#[test]
fn a_retry_after_a_lost_answer_is_not_received_twice() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    sees(&laptop, &phone);
    let all_done = || {
        laptop
            .transfers()
            .iter()
            .filter(|t| t.direction == Direction::Sent)
            .all(|t| t.status == TransferStatus::Done)
    };
    // The phone saves the item, but the laptop doesn't hear it did, and
    // tries again a second later.
    laptop.lose_next_answers(1);
    let sent = block_on(laptop.send_note_to_device(phone_id.clone(), "once".into())).unwrap();
    assert_eq!(sent.status, TransferStatus::Waiting);
    eventually("the note done on the laptop", all_done);
    laptop.lose_next_answers(1);
    let map = dir.path().join("map.txt");
    std::fs::write(&map, b"north").unwrap();
    let sent = block_on(laptop.send_to_device(phone_id, vec![map.to_string_lossy().into_owned()]))
        .unwrap();
    assert_eq!(sent.status, TransferStatus::Waiting);
    eventually("both done on the laptop", || {
        laptop
            .transfers()
            .iter()
            .filter(|t| t.direction == Direction::Sent)
            .all(|t| t.status == TransferStatus::Done)
    });
    let got = received(&phone);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0], note("once"));
    assert!(matches!(&got[1], Item::File { name, .. } if name == "map.txt"));
}

#[test]
fn a_waiting_item_can_be_cancelled_and_a_revoked_device_ends_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    drop(phone);
    let cancelled = block_on(laptop.send_note_to_device(phone_id.clone(), "no".into())).unwrap();
    let kept = block_on(laptop.send_note_to_device(phone_id.clone(), "yes".into())).unwrap();
    laptop.cancel_transfer(cancelled.id);
    let status = |id: u64| {
        laptop
            .transfers()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
            .status
    };
    eventually("the cancel", || {
        status(cancelled.id)
            == TransferStatus::Failed {
                reason: "Cancelled".into(),
            }
    });
    assert_eq!(status(kept.id), TransferStatus::Waiting);
    block_on(laptop.revoke_device(phone_id)).unwrap();
    eventually("no longer trusted", || {
        status(kept.id)
            == TransferStatus::Failed {
                reason: "No longer trusted".into(),
            }
    });
}
