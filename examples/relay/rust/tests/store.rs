//! Relay's one database (review P3): the JSON files of an older build move
//! in once, a second open changes nothing, and saved data that can't be
//! read is set aside while the device keeps its key.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::{Direction, RelayCore};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

/// Opens Relay in `dir`, waiting while a closing one still holds it.
fn open(dir: &Path) -> Arc<RelayCore> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match RelayCore::new(dir.to_string_lossy().into_owned(), "laptop".into()) {
            Ok(core) => return core,
            Err(err) => {
                assert!(Instant::now() < deadline, "couldn't open Relay: {err}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Closes `core` and waits until it has let go of its folder.
fn close(core: Arc<RelayCore>) {
    core.shutdown();
    let closing = Arc::downgrade(&core);
    drop(core);
    let deadline = Instant::now() + Duration::from_secs(15);
    while closing.strong_count() > 0 {
        assert!(Instant::now() < deadline, "Relay didn't close");
        std::thread::sleep(Duration::from_millis(20));
    }
}

const PHONE: &str = "ae9c1fd2b8f0c6e4d3a2b1c0f9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e0";

/// What an older build saved: its record and one device conversation.
fn older_build(dir: &Path) {
    std::fs::create_dir_all(dir.join("devices").join(PHONE)).unwrap();
    let record = serde_json::json!({
        "schema_version": 1,
        "data": {
            "device_id": "0011223344556677",
            "name": "laptop",
            "next_id": 3,
            "notes_are_messages": true,
            "devices": {
                "policy": "AskToPair",
                "list": [{
                    "endpoint_id": PHONE,
                    "name": "Phone",
                    "trust": "Trusted",
                    "since_ms": 1000,
                    "receives_from": true
                }]
            },
            "transfers": [{
                "id": 1, "peer_id": PHONE, "peer_name": "Phone", "sent": false,
                "item": { "File": { "name": "map.txt", "size": 5, "file": null } },
                "status": "Done", "at_ms": 2000
            }, {
                "id": 2, "peer_id": PHONE, "peer_name": "Phone", "sent": true,
                "item": { "File": { "name": "big.bin", "size": 9, "file": null } },
                "status": "Waiting", "at_ms": 3000
            }],
            "deliveries": [{
                "id": "00112233445566778899aabbccddeeff", "to": PHONE, "transfer": 2,
                "payload": { "Files": {
                    "hash": "5".repeat(64), "collection": false, "name": "big.bin",
                    "size": 9, "hashes": ["5".repeat(64)]
                } },
                "queued_ms": 3000
            }],
            "received_ids": [{ "from": PHONE, "id": "ab".repeat(16), "at_ms": 2000 }]
        }
    });
    std::fs::write(dir.join("relay.json"), record.to_string()).unwrap();
    let message = |id: &str, body: &str, at: i64, status: &str, author: &str| {
        serde_json::json!({
            "id": id, "conversation_id": format!("device:{PHONE}"), "author": author,
            "author_id": author, "body": body, "sent_at": at, "status": status
        })
    };
    let log = serde_json::json!({
        "schema_version": 1,
        "data": {
            "messages": [
                message("m1", "hello", 1500, "sent", PHONE),
                message("m2", "back", 1600, "sent", "relay:0011223344556677"),
                // Saved queued, but its delivery isn't in the outbox (the
                // older build ended between its two saves): it failed.
                message("m3", "lost", 1700, "queued", "relay:0011223344556677"),
            ],
            "last_at": 1700,
            "files": {}
        }
    });
    std::fs::write(
        dir.join("devices").join(PHONE).join("messages.json"),
        log.to_string(),
    )
    .unwrap();
}

fn bodies(core: &RelayCore) -> Vec<(String, chat_core::MessageStatus)> {
    core.device_messages(PHONE)
        .unwrap()
        .into_iter()
        .map(|m| (m.body, m.status))
        .collect()
}

#[test]
fn an_older_builds_files_move_in_once() {
    use chat_core::MessageStatus::{Failed, Sent};
    let dir = tempfile::tempdir().unwrap();
    older_build(dir.path());

    let core = open(dir.path());
    assert_eq!(core.identity().device_id, "0011223344556677");
    let devices = core.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "Phone");
    assert!(devices[0].trusted);
    let history = core.transfers();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].direction, Direction::Received);
    assert_eq!(core.saved_data_set_aside(), None);
    let messages = vec![
        ("hello".to_string(), Sent),
        ("back".to_string(), Sent),
        ("lost".to_string(), Failed),
    ];
    assert_eq!(bodies(&core), messages);
    // The JSON files are kept, renamed, never read again.
    assert!(!dir.path().join("relay.json").exists());
    assert!(dir.path().join("relay.json.migrated").exists());
    let log = dir.path().join("devices").join(PHONE);
    assert!(log.join("messages.json.migrated").exists());
    assert!(dir.path().join("relay.db").exists());

    // A change, then a second open: what was saved, once.
    block_on(core.rename_device("create".into())).unwrap();
    close(core);
    let core = open(dir.path());
    assert_eq!(core.identity().name, "create");
    assert_eq!(core.devices().len(), 1);
    assert_eq!(core.transfers().len(), 2);
    assert_eq!(bodies(&core), messages);
    close(core);
}

#[test]
fn json_files_left_behind_are_not_read_again() {
    let dir = tempfile::tempdir().unwrap();
    older_build(dir.path());
    let core = open(dir.path());
    close(core);
    // As if Relay ended after the database was written but before the
    // files were renamed: the database wins, nothing is added twice.
    std::fs::rename(
        dir.path().join("relay.json.migrated"),
        dir.path().join("relay.json"),
    )
    .unwrap();
    let core = open(dir.path());
    assert_eq!(core.devices().len(), 1);
    assert_eq!(core.device_messages(PHONE).unwrap().len(), 3);
    assert!(!dir.path().join("relay.json").exists(), "renamed this time");
    close(core);
}

#[test]
fn an_unreadable_record_is_set_aside_and_the_key_kept() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    let id = block_on(core.device_service()).unwrap().endpoint_id;
    close(core);

    // The database is damaged.
    std::fs::write(
        dir.path().join("relay.db"),
        b"not a database at all, not even close",
    )
    .unwrap();
    for extra in ["relay.db-wal", "relay.db-shm"] {
        let _ = std::fs::remove_file(dir.path().join(extra));
    }
    let core = open(dir.path());
    let kept = core.saved_data_set_aside().expect("it says where it went");
    assert!(Path::new(&kept).exists(), "kept, not deleted");
    assert!(core.devices().is_empty());
    assert_eq!(
        block_on(core.device_service()).unwrap().endpoint_id,
        id,
        "the same device"
    );
    close(core);

    // An older build's record that doesn't parse.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("relay.json"),
        b"{ \"schema_version\": 1, \"data\": [",
    )
    .unwrap();
    let core = open(dir.path());
    let kept = core.saved_data_set_aside().expect("it says where it went");
    assert!(kept.contains("relay.json.unreadable"), "{kept}");
    assert!(Path::new(&kept).exists());
    assert!(!core.identity().device_id.is_empty());
    close(core);
}

#[test]
fn a_record_from_a_newer_build_is_refused_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("relay.json"),
        br#"{ "schema_version": 9, "data": {} }"#,
    )
    .unwrap();
    let err = RelayCore::new(dir.path().to_string_lossy().into_owned(), "laptop".into())
        .err()
        .expect("refused");
    assert_eq!(err, relay_core::RelayError::NewerData);
    assert!(dir.path().join("relay.json").exists(), "left as it was");
}
