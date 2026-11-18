//! The connection resolver (Relay M2 PR 6): a trusted device is found
//! again without Bonjour, on the port it keeps across relaunches, each
//! connection reports its route, and when a device doesn't answer Relay
//! says why in words. Two Relay devices on one machine.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{
    CodeSend, ConnectionMode, Device, Direction, Item, NearbyDevice, RelayCore, TransferStatus,
};

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

/// Notes `core` received from the device called `from`, oldest first.
fn notes_from(core: &RelayCore, from: &str) -> Vec<String> {
    core.transfers()
        .into_iter()
        .rev()
        .filter(|t| t.direction == Direction::Received && t.peer_name == from)
        .filter(|t| t.status == TransferStatus::Done)
        .filter_map(|t| match t.item {
            Item::Note { text } => Some(text),
            _ => None,
        })
        .collect()
}

fn laptop_on(phone: &RelayCore) -> Device {
    phone
        .devices()
        .into_iter()
        .find(|d| d.name == "laptop")
        .expect("the laptop is trusted on the phone")
}

#[test]
fn a_device_is_found_again_without_bonjour_and_says_why_when_it_isnt() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, _) = trusted_pair(dir.path());
    let laptop_id = laptop_on(&phone).endpoint_id;
    let service = block_on(laptop.device_service()).unwrap();
    let port = service.port;
    // What it tells devices it meets: its interfaces' addresses on its port,
    // never loopback.
    assert!(
        service
            .addresses
            .iter()
            .all(|a| a.ends_with(&format!(":{port}")) && !a.starts_with("127.")),
        "{:?}",
        service.addresses
    );

    // Together: the phone sees the laptop with Bonjour, and a note goes.
    sees(&phone, &laptop);
    block_on(phone.send_note_to_device(laptop_id.clone(), "one".into())).unwrap();
    eventually("the first note", || notes_from(&laptop, "phone") == ["one"]);
    eventually("the route", || {
        laptop_on(&phone).route.as_deref() == Some("Direct, local network")
    });
    assert_eq!(laptop_on(&phone).problem, None);

    // Bonjour no longer sees it, and Relay closes on the laptop.
    phone.devices_nearby(vec![]);
    drop(laptop);
    std::thread::sleep(Duration::from_millis(500));
    let sent = block_on(phone.send_note_to_device(laptop_id.clone(), "two".into())).unwrap();
    assert_eq!(sent.status, TransferStatus::Waiting);
    eventually("the reason, in words", || {
        laptop_on(&phone)
            .problem
            .is_some_and(|p| p.contains("laptop didn't answer"))
    });

    // Relay opens on the laptop again, on the same port: the phone finds
    // it where it was, though Bonjour still sees nothing.
    let laptop = device(dir.path(), "laptop");
    assert_eq!(block_on(laptop.device_service()).unwrap().port, port);
    phone.deliver_waiting();
    eventually("the second note", || {
        notes_from(&laptop, "phone") == ["one", "two"]
    });
    eventually("the reason gone", || laptop_on(&phone).problem.is_none());

    // Local network only still reaches it (it is on this machine), and the
    // setting lasts.
    block_on(phone.set_connection_mode(ConnectionMode::Local)).unwrap();
    assert_eq!(phone.connection_mode(), ConnectionMode::Local);
    block_on(phone.send_note_to_device(laptop_id, "three".into())).unwrap();
    eventually("the third note", || {
        notes_from(&laptop, "phone") == ["one", "two", "three"]
    });
}
