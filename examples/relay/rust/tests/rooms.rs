//! Rooms over `relay/room/1` from Relay's node (N4 step 3): a device joins
//! a host's rooms by word code, and the chat repo works over the room
//! transport as over HTTP, the live stream included. The host (the real
//! chat server) and two devices run in process.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chat_core::{ChatRepo, OutgoingMessage, RemoteEvent};
use chat_server::{room, AppState};
use forge_net::CancelHandle;
use iroh::endpoint::presets;
use iroh::{Endpoint, RelayMode, SecretKey};
use relay_core::rooms::{join, room_repo, RoomHost};
use relay_core::{RelayCore, RelayServers};

fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let core = RelayCore::with_default_servers(
        dir.join(name).to_string_lossy().into_owned(),
        name.into(),
        RelayServers::default(),
    )
    .unwrap();
    forge_runtime::block_on(core.device_service()).unwrap();
    core
}

fn outgoing(id: &str, body: &str) -> OutgoingMessage {
    OutgoingMessage {
        id: id.into(),
        conversation_id: "general".into(),
        author: String::new(),
        author_id: String::new(),
        body: body.into(),
        reply_to: None,
        attachments: Vec::new(),
    }
}

#[test]
fn devices_join_a_room_by_code_and_talk_live_over_relay() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::open(dir.path().join("server.json"), Duration::from_millis(200)).unwrap();
    let (host, port) = rt.block_on(async {
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::from_bytes(&[42; 32]))
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .unwrap();
        let port = endpoint.bound_sockets()[0].port();
        (room::serve(state.clone(), endpoint), port)
    });
    let where_ = RoomHost {
        endpoint_id: host.endpoint().id().to_string(),
        relay_url: None,
        addresses: vec![format!("127.0.0.1:{port}")],
    };

    let phone = device(dir.path(), "phone");
    let laptop = device(dir.path(), "laptop");
    // A wrong code is refused; the right one, once, makes an account
    // that is the device's key.
    let wrong = join(&phone, where_.clone(), "not-the-right-code", "Ana");
    assert!(
        matches!(wrong, Err(chat_core::ChatError::Http { status: 403, .. })),
        "{wrong:?}"
    );
    let ana = join(&phone, where_.clone(), &state.invite().unwrap(), "Ana").unwrap();
    assert_eq!(ana.display_name, "Ana");
    let again = join(&phone, where_.clone(), "anything", "Ana").unwrap();
    assert_eq!(again, ana, "already in: the same account");
    let bo = join(&laptop, where_.clone(), &state.invite().unwrap(), "Bo").unwrap();
    assert_ne!(bo.user_id, ana.user_id);

    let on_phone = room_repo(&phone, where_.clone());
    let on_laptop = room_repo(&laptop, where_.clone());
    assert_eq!(
        on_phone.identity().unwrap().user_id,
        ana.user_id,
        "no token: the key"
    );

    // The phone listens; the laptop posts; it arrives live.
    let cancel = CancelHandle::new();
    let mut stream = on_phone.open_stream("general", &cancel).unwrap();
    let posted = on_laptop
        .post_message(&outgoing("m1", "hello over relay"))
        .unwrap();
    assert_eq!(posted.author_id, bo.user_id);
    let deadline = Instant::now() + Duration::from_secs(20);
    let arrived = loop {
        assert!(Instant::now() < deadline, "nothing arrived on the stream");
        match stream.next_event() {
            Some(Ok(RemoteEvent::Message(m))) if m.id == "m1" => break m,
            Some(Ok(_)) => continue,
            other => panic!("the stream ended: {other:?}"),
        }
    };
    assert_eq!(arrived.body, "hello over relay");
    assert!(on_phone
        .list_conversations()
        .unwrap()
        .iter()
        .any(|c| c.id == "general"));

    // Cancelling ends the stream promptly.
    cancel.cancel();
    let started = Instant::now();
    while let Some(Ok(_)) = stream.next_event() {}
    assert!(started.elapsed() < Duration::from_secs(5));

    phone.shutdown();
    laptop.shutdown();
    rt.block_on(host.shutdown()).unwrap();
}
