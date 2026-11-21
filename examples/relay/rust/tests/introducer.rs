//! My Devices, step 1 (review P5): one trusted device introduces the others.
//! Pair a device once, with the introducer, and your other devices are
//! offered it; revoke it once, on the introducer, and they revoke it too.
//! Three Relay devices on one machine, over localhost.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{CodeSend, NearbyDevice, RelayCore, RelayError};

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

fn id(core: &RelayCore) -> String {
    block_on(core.device_service()).unwrap().endpoint_id
}

/// `a` and `b` make one code transfer and trust each other.
fn pair(dir: &Path, a: &RelayCore, b: &RelayCore) {
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    a.send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(a.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = a.code_send_status()
    else {
        unreachable!()
    };
    block_on(b.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id)).unwrap();
    eventually("both offers", || {
        a.trust_offer().is_some() && b.trust_offer().is_some()
    });
    for core in [a, b] {
        let offer = core.trust_offer().unwrap();
        block_on(core.trust_device(offer.endpoint_id)).unwrap();
    }
}

/// Tells `core` where `others` are, as Bonjour would (the whole list).
fn sees(core: &RelayCore, others: &[&RelayCore]) {
    let list = others
        .iter()
        .map(|other| {
            let service = block_on(other.device_service()).unwrap();
            NearbyDevice {
                endpoint_id: service.endpoint_id,
                addresses: vec![format!("127.0.0.1:{}", service.port)],
            }
        })
        .collect();
    core.devices_nearby(list);
}

/// A laptop paired with a phone and a tablet, which haven't met. Each of
/// them lets the laptop introduce devices, and the laptop lets each see
/// its devices.
fn introduced_by_the_laptop(
    dir: &Path,
) -> (Arc<RelayCore>, Arc<RelayCore>, Arc<RelayCore>, String) {
    let laptop = device(dir, "laptop");
    let phone = device(dir, "phone");
    let tablet = device(dir, "tablet");
    pair(dir, &laptop, &phone);
    pair(dir, &laptop, &tablet);
    let laptop_id = id(&laptop);
    for other in [&phone, &tablet] {
        sees(other, &[&laptop]);
        block_on(laptop.set_sees_devices(id(other), true)).unwrap();
        block_on(other.set_introduces(laptop_id.clone(), true)).unwrap();
    }
    (laptop, phone, tablet, laptop_id)
}

#[test]
fn a_device_paired_once_is_offered_to_your_other_devices() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, tablet, laptop_id) = introduced_by_the_laptop(dir.path());

    // The phone hears of the tablet from the laptop: an offer, never trust
    // by itself.
    block_on(phone.sync_devices(laptop_id.clone())).unwrap();
    let offer = phone.trust_offer().expect("the tablet is offered");
    assert_eq!(offer.endpoint_id, id(&tablet));
    assert_eq!(offer.name, "tablet");
    assert_eq!(offer.introduced_by.as_deref(), Some("laptop"));
    assert!(!phone.devices().iter().any(|d| d.name == "tablet"));

    // The person accepts; the phone remembers who introduced it.
    block_on(phone.trust_device(offer.endpoint_id.clone())).unwrap();
    let on_phone = phone.devices();
    let t = on_phone.iter().find(|d| d.name == "tablet").unwrap();
    assert!(t.trusted);
    assert_eq!(t.introduced_by.as_deref(), Some("laptop"));
    assert!(
        !t.introduces && !t.sees_devices,
        "trust grants nothing more"
    );

    // The tablet hears of the phone the same way, and the two talk.
    block_on(tablet.sync_devices(laptop_id.clone())).unwrap();
    let offer = tablet.trust_offer().expect("the phone is offered");
    assert_eq!(offer.endpoint_id, id(&phone));
    block_on(tablet.trust_device(offer.endpoint_id)).unwrap();
    sees(&phone, &[&laptop, &tablet]);
    let status = block_on(phone.device_status(id(&tablet))).unwrap();
    assert_eq!(status.name, "tablet");

    // Asking again offers nothing new.
    let again = block_on(phone.sync_devices(laptop_id)).unwrap();
    assert_eq!((again.offered, again.revoked), (0, 0));
    assert!(phone.trust_offer().is_none());
}

#[test]
fn a_device_revoked_once_is_revoked_on_your_other_devices() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, tablet, laptop_id) = introduced_by_the_laptop(dir.path());
    block_on(phone.sync_devices(laptop_id.clone())).unwrap();
    let tablet_id = phone.trust_offer().unwrap().endpoint_id;
    block_on(phone.trust_device(tablet_id.clone())).unwrap();

    // Revoked on the laptop; the phone follows, and says who did it.
    block_on(laptop.revoke_device(tablet_id.clone())).unwrap();
    // A sync the phone starts by itself (on opening the device service)
    // may get there first, so the count is 1 or 0 and the outcome is
    // what's checked.
    let synced = block_on(phone.sync_devices(laptop_id.clone())).unwrap();
    assert_eq!(synced.offered, 0);
    assert!(synced.revoked <= 1);
    let tablet_on_phone = || {
        phone
            .devices()
            .into_iter()
            .find(|d| d.endpoint_id == tablet_id)
            .unwrap()
    };
    eventually("the phone revokes the tablet", || {
        !tablet_on_phone().trusted
    });
    assert_eq!(tablet_on_phone().revoked_by.as_deref(), Some("laptop"));

    // The tablet is refused by the phone now.
    sees(&tablet, &[&laptop, &phone]);
    block_on(tablet.trust_device(id(&phone))).ok();
    assert!(block_on(tablet.device_status(id(&phone))).is_err());

    // Revoked here, it isn't offered again; only the person trusts it
    // again, and that clears who revoked it.
    block_on(phone.sync_devices(laptop_id)).unwrap();
    assert!(phone.trust_offer().is_none());
    block_on(phone.trust_device(tablet_id.clone())).unwrap();
    let t = phone
        .devices()
        .into_iter()
        .find(|d| d.endpoint_id == tablet_id)
        .unwrap();
    assert!(t.trusted);
    assert_eq!(t.revoked_by, None);
}

#[test]
fn the_list_takes_both_switches() {
    let dir = tempfile::tempdir().unwrap();
    let (laptop, phone, _tablet, laptop_id) = introduced_by_the_laptop(dir.path());

    // The laptop no longer lets the phone see its devices: refused there.
    block_on(laptop.set_sees_devices(id(&phone), false)).unwrap();
    assert!(matches!(
        block_on(phone.sync_devices(laptop_id.clone())),
        Err(RelayError::Declined { .. })
    ));

    // The phone no longer lets the laptop introduce: not even asked.
    block_on(laptop.set_sees_devices(id(&phone), true)).unwrap();
    block_on(phone.set_introduces(laptop_id.clone(), false)).unwrap();
    assert!(matches!(
        block_on(phone.sync_devices(laptop_id)),
        Err(RelayError::Declined { .. })
    ));
    assert!(phone.trust_offer().is_none());
}
