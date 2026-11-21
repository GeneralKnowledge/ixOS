//! Away from home (N2): Relay's servers on by default, unless the person
//! chose otherwise, and an upgrade that turns them on says so once.

use std::path::Path;
use std::sync::Arc;

use relay_core::{ConnectionMode, RelayCore, RelayServers};

const PHONE: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

/// Servers nothing reaches, so no test ever talks to the real ones.
fn defaults() -> RelayServers {
    RelayServers {
        relay_url: Some("https://relay.invalid".into()),
        lookup_url: Some("https://lookup.invalid/pkarr".into()),
    }
}

fn open(dir: &Path) -> Arc<RelayCore> {
    RelayCore::with_default_servers(
        dir.to_string_lossy().into_owned(),
        "laptop".into(),
        defaults(),
    )
    .unwrap()
}

fn close(core: Arc<RelayCore>) {
    core.shutdown();
    let closing = Arc::downgrade(&core);
    drop(core);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while closing.strong_count() > 0 {
        assert!(std::time::Instant::now() < deadline, "Relay didn't close");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Relay as an older build left it: one trusted device, and `devices`
/// as given (mode, servers).
fn older_build(dir: &Path, devices: serde_json::Value) {
    let mut devices = devices;
    devices["list"] = serde_json::json!([{
        "endpoint_id": PHONE,
        "name": "Phone",
        "trust": "Trusted",
        "since_ms": 1000,
        "receives_from": true
    }]);
    let record = serde_json::json!({
        "schema_version": 1,
        "data": { "name": "laptop", "next_id": 1, "notes_are_messages": true, "devices": devices }
    });
    std::fs::write(dir.join("relay.json"), record.to_string()).unwrap();
}

#[test]
fn a_new_device_uses_relays_servers_without_a_word() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    assert_eq!(core.servers(), defaults());
    assert_eq!(core.connection_mode(), ConnectionMode::Hybrid);
    assert!(
        core.away_from_home_notice().is_none(),
        "nothing changed for them"
    );
    close(core);
}

#[test]
fn an_upgrade_that_never_chose_turns_them_on_and_says_so_once() {
    let dir = tempfile::tempdir().unwrap();
    older_build(dir.path(), serde_json::json!({ "policy": "AskToPair" }));
    let core = open(dir.path());
    assert_eq!(core.servers(), defaults());
    assert!(core
        .away_from_home_notice()
        .unwrap()
        .contains("away from home"));
    // Until dismissed, also after a relaunch; then never again.
    close(core);
    let core = open(dir.path());
    assert!(core.away_from_home_notice().is_some());
    forge_runtime::block_on(core.dismiss_away_from_home_notice()).unwrap();
    close(core);
    let core = open(dir.path());
    assert!(core.away_from_home_notice().is_none());
    close(core);
}

#[test]
fn a_person_who_chose_private_or_their_own_servers_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    older_build(
        dir.path(),
        serde_json::json!({ "policy": "AskToPair", "mode": "Private" }),
    );
    let core = open(dir.path());
    assert_eq!(core.servers(), RelayServers::default());
    assert_eq!(core.connection_mode(), ConnectionMode::Private);
    assert!(core.away_from_home_notice().is_none());
    close(core);

    let dir = tempfile::tempdir().unwrap();
    let own = RelayServers {
        relay_url: Some("https://relay.example.invalid".into()),
        lookup_url: None,
    };
    older_build(
        dir.path(),
        serde_json::json!({ "policy": "AskToPair", "servers": own }),
    );
    let core = open(dir.path());
    assert_eq!(core.servers(), own);
    assert!(core.away_from_home_notice().is_none());
    close(core);
}

#[test]
fn turning_them_off_is_never_undone() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    forge_runtime::block_on(core.set_servers(RelayServers::default())).unwrap();
    close(core);
    let core = open(dir.path());
    assert_eq!(core.servers(), RelayServers::default());
    close(core);
}
