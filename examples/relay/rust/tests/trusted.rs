//! Trusted devices (brief PR 3): trust after a code transfer, sending
//! without a code over `relay/device/1`, and the revocation rule. Two Relay
//! devices on one machine, over localhost.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{
    CodeSend, Item, NearbyDevice, PairingPolicy, Presence, RelayCore, RelayError, TransferStatus,
};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let core = RelayCore::with_default_servers(
        dir.join(name).to_string_lossy().into_owned(),
        name.into(),
        relay_core::RelayServers::default(),
    )
    .unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(15);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `from` sends `file` to `to` with a word code; returns `to`'s receipt.
fn code_transfer(from: &RelayCore, to: &RelayCore, file: &Path) -> relay_core::CodeReceipt {
    from.send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(from.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = from.code_send_status()
    else {
        unreachable!()
    };
    let receipt =
        block_on(to.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id))
            .unwrap();
    eventually("the send to end", || {
        matches!(from.code_send_status(), CodeSend::Sent { .. })
    });
    receipt
}

/// Tells `core` where `other` is, as Bonjour would.
fn sees(core: &RelayCore, other: &RelayCore) -> String {
    let service = block_on(other.device_service()).unwrap();
    core.devices_nearby(vec![NearbyDevice {
        endpoint_id: service.endpoint_id.clone(),
        addresses: vec![format!("127.0.0.1:{}", service.port)],
    }]);
    service.endpoint_id
}

/// A phone and a laptop that made one code transfer and trusted each other.
fn trusted_pair(dir: &Path) -> (Arc<RelayCore>, Arc<RelayCore>, String, String) {
    let phone = device(dir, "phone");
    let laptop = device(dir, "laptop");
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    code_transfer(&laptop, &phone, &file);
    eventually("the laptop's trust offer", || {
        laptop.trust_offer().is_some()
    });
    let on_phone = phone.trust_offer().expect("the phone's trust offer");
    let on_laptop = laptop.trust_offer().unwrap();
    assert_eq!(on_phone.name, "laptop");
    assert_eq!(on_laptop.name, "phone");
    block_on(phone.trust_device(on_phone.endpoint_id.clone())).unwrap();
    block_on(laptop.trust_device(on_laptop.endpoint_id.clone())).unwrap();
    assert!(phone.trust_offer().is_none());
    (phone, laptop, on_laptop.endpoint_id, on_phone.endpoint_id)
}

#[test]
fn after_a_code_transfer_both_sides_are_asked_and_the_receipt_names_the_sender() {
    let dir = tempfile::tempdir().unwrap();
    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    let file = dir.path().join("a.txt");
    std::fs::write(&file, b"a").unwrap();
    let receipt = code_transfer(&laptop, &phone, &file);
    assert_eq!(receipt.sender_name, "laptop");
    assert_eq!(
        receipt.sender_id,
        block_on(laptop.device_service()).unwrap().endpoint_id
    );
    eventually("the laptop's offer", || laptop.trust_offer().is_some());
    assert_eq!(phone.trust_offer().unwrap().endpoint_id, receipt.sender_id);
    // Not Now leaves nothing behind.
    phone.dismiss_trust_offer();
    assert!(phone.trust_offer().is_none());
    assert!(phone.devices().is_empty());
}

#[test]
fn a_second_code_transfer_doesnt_replace_the_first_offer() {
    let dir = tempfile::tempdir().unwrap();
    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    let tablet = device(dir.path(), "tablet");
    let file = dir.path().join("a.txt");
    std::fs::write(&file, b"a").unwrap();
    let from_laptop = code_transfer(&laptop, &phone, &file);
    let from_tablet = code_transfer(&tablet, &phone, &file);
    // The oldest is offered first; once it is answered, the next one is.
    assert_eq!(
        phone.trust_offer().unwrap().endpoint_id,
        from_laptop.sender_id
    );
    phone.dismiss_trust_offer();
    let next = phone
        .trust_offer()
        .expect("the tablet's offer is still there");
    assert_eq!(next.endpoint_id, from_tablet.sender_id);
    assert_eq!(next.name, "tablet");
    // Trusting a device takes its own offer, wherever it is in the line.
    code_transfer(&laptop, &phone, &file);
    block_on(phone.trust_device(from_laptop.sender_id.clone())).unwrap();
    assert_eq!(
        phone.trust_offer().unwrap().endpoint_id,
        from_tablet.sender_id
    );
    block_on(phone.trust_device(from_tablet.sender_id.clone())).unwrap();
    assert!(phone.trust_offer().is_none());
    assert_eq!(phone.devices().len(), 2);
}

#[test]
fn codes_only_never_asks() {
    let dir = tempfile::tempdir().unwrap();
    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    block_on(phone.set_pairing_policy(PairingPolicy::CodesOnly)).unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, b"a").unwrap();
    code_transfer(&laptop, &phone, &file);
    eventually("the laptop's offer", || laptop.trust_offer().is_some());
    assert!(phone.trust_offer().is_none());
}

#[test]
fn trusted_devices_send_notes_and_folders_without_a_code_and_survive_a_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id, laptop_id) = trusted_pair(dir.path());
    drop(phone);
    drop(laptop);
    std::thread::sleep(Duration::from_millis(500));
    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    assert_eq!(phone.devices().len(), 1);
    assert!(phone.devices()[0].trusted);
    assert_eq!(laptop.devices()[0].endpoint_id, phone_id);

    sees(&laptop, &phone);
    assert!(laptop.devices()[0].nearby);
    let note = block_on(laptop.send_note_as_older_build(phone_id.clone(), "hi".into())).unwrap();
    assert_eq!(note.status, TransferStatus::Done);
    // A note arrives as a message in the conversation, not in history.
    eventually("the note on the phone", || {
        !phone.device_messages(&laptop_id).unwrap().is_empty()
    });
    let got = &phone.device_messages(&laptop_id).unwrap()[0];
    assert_eq!(got.author, "laptop");
    assert_eq!(got.body, "hi");

    let trip = dir.path().join("Trip");
    std::fs::create_dir_all(trip.join("day 1")).unwrap();
    std::fs::write(trip.join("day 1/map.txt"), b"north").unwrap();
    let sent = block_on(
        laptop.send_to_device(phone_id.clone(), vec![trip.to_string_lossy().into_owned()]),
    )
    .unwrap();
    assert_eq!(sent.status, TransferStatus::Done);
    let received = phone
        .transfers()
        .into_iter()
        .find(|t| matches!(&t.item, Item::File { name, .. } if name == "Trip"))
        .expect("the folder in the phone's history");
    assert_eq!(received.status, TransferStatus::Done);
    let Item::File {
        path: Some(path), ..
    } = received.item
    else {
        panic!("no path")
    };
    assert_eq!(
        std::fs::read(Path::new(&path).join("day 1/map.txt")).unwrap(),
        b"north"
    );
    // The other way too.
    sees(&phone, &laptop);
    block_on(phone.send_note_as_older_build(laptop_id, "back".into())).unwrap();
}

#[test]
fn a_revoked_device_is_refused_and_codes_never_restore_it() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id, laptop_id) = trusted_pair(dir.path());
    block_on(phone.set_pairing_policy(PairingPolicy::PairAutomatically)).unwrap();
    block_on(phone.revoke_device(laptop_id.clone())).unwrap();
    assert!(!phone.devices()[0].trusted);

    // Its trusted connection is refused.
    sees(&laptop, &phone);
    let refused = block_on(laptop.send_note_as_older_build(phone_id.clone(), "hi".into()));
    assert_eq!(refused.unwrap_err(), RelayError::Refused);
    assert!(phone
        .transfers()
        .iter()
        .all(|t| !matches!(&t.item, Item::Note { .. })));

    // A one-off code transfer still works, and leaves it revoked, with no
    // offer, even under Pair Automatically and after forgetting it.
    block_on(phone.forget_device(laptop_id.clone())).unwrap();
    let file = dir.path().join("again.txt");
    std::fs::write(&file, b"again").unwrap();
    let receipt = code_transfer(&laptop, &phone, &file);
    assert_eq!(std::fs::read(&receipt.path).unwrap(), b"again");
    assert!(phone.trust_offer().is_none());
    let refused = block_on(laptop.send_note_as_older_build(phone_id.clone(), "hi".into()));
    assert_eq!(refused.unwrap_err(), RelayError::Refused);

    // Only the person trusts it again.
    block_on(phone.trust_device(laptop_id)).unwrap();
    block_on(laptop.send_note_as_older_build(phone_id, "hi".into())).unwrap();
}

#[test]
fn pair_automatically_trusts_a_forgotten_device_again() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id, laptop_id) = trusted_pair(dir.path());
    block_on(phone.forget_device(laptop_id.clone())).unwrap();
    assert!(phone.devices().is_empty());
    sees(&laptop, &phone);
    let refused = block_on(laptop.send_note_as_older_build(phone_id.clone(), "hi".into()));
    assert_eq!(refused.unwrap_err(), RelayError::Refused);

    block_on(phone.set_pairing_policy(PairingPolicy::PairAutomatically)).unwrap();
    let file = dir.path().join("b.txt");
    std::fs::write(&file, b"b").unwrap();
    code_transfer(&laptop, &phone, &file);
    assert!(
        phone.trust_offer().is_none(),
        "trusted again without asking"
    );
    assert!(phone.devices()[0].trusted);
    block_on(laptop.send_note_as_older_build(phone_id, "hi".into())).unwrap();
}

#[test]
fn a_device_that_may_not_send_here_is_declined() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id, laptop_id) = trusted_pair(dir.path());
    block_on(phone.set_receives_from(laptop_id, false)).unwrap();
    sees(&laptop, &phone);
    let declined = block_on(laptop.send_note_as_older_build(phone_id, "hi".into()));
    assert!(
        matches!(declined, Err(RelayError::Declined { .. })),
        "{declined:?}"
    );
}

#[test]
fn only_trusted_devices_can_be_sent_to() {
    let dir = tempfile::tempdir().unwrap();
    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    let phone_id = sees(&laptop, &phone);
    // The laptop never trusted the phone.
    assert!(matches!(
        block_on(laptop.send_note_as_older_build(phone_id, "hi".into())),
        Err(RelayError::UnknownPeer { .. })
    ));
}

#[test]
fn a_trusted_device_says_what_it_is_and_shows_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id, laptop_id) = trusted_pair(dir.path());
    // Nothing reached yet over relay/device/1, and Bonjour sees nothing.
    let before = &laptop.devices()[0];
    assert_eq!(before.platform, None);
    assert!(before.offers.is_empty());

    // Bonjour sees the phone: nearby, and its status can be asked.
    sees(&laptop, &phone);
    assert_eq!(laptop.devices()[0].presence, Presence::Nearby);
    let asked = block_on(laptop.device_status(phone_id.clone())).unwrap();
    assert_eq!(asked.platform.as_deref(), Some(std::env::consts::OS));
    for offer in ["notes", "files", "messages"] {
        assert!(
            asked.offers.iter().any(|o| o == offer),
            "{offer} in {:?}",
            asked.offers
        );
    }
    assert!(asked.last_seen_ms.is_some());

    // The phone was asked over a trusted connection: the laptop reached it,
    // so the phone shows it as reachable though Bonjour never saw it.
    let on_phone = phone
        .devices()
        .into_iter()
        .find(|d| d.endpoint_id == laptop_id)
        .unwrap();
    assert_eq!(on_phone.presence, Presence::Reachable);
    assert!(on_phone.last_seen_ms.is_some());

    // Bonjour loses the phone: reachable for a while, not nearby.
    laptop.devices_nearby(vec![]);
    assert_eq!(laptop.devices()[0].presence, Presence::Reachable);

    // What it said is kept across a relaunch.
    drop(laptop);
    std::thread::sleep(Duration::from_millis(500));
    let laptop = device(dir.path(), "laptop");
    let kept = &laptop.devices()[0];
    assert_eq!(kept.platform.as_deref(), Some(std::env::consts::OS));
    assert!(kept.last_seen_ms.is_some());

    // A revoked device can't ask, and shows as offline where it was revoked.
    block_on(phone.revoke_device(laptop_id.clone())).unwrap();
    sees(&laptop, &phone);
    let refused = block_on(laptop.device_status(phone_id));
    assert_eq!(refused.unwrap_err(), RelayError::Refused);
    let revoked = &phone.devices()[0];
    assert!(!revoked.trusted);
    assert_eq!(revoked.presence, Presence::Offline);
    // And nobody can ask about a device that isn't trusted here.
    assert!(matches!(
        block_on(phone.device_status(laptop_id)),
        Err(RelayError::UnknownPeer { .. })
    ));
}
