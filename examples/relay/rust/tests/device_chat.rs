//! Device conversations: each trusted device is a conversation in Chats
//! on chat-core, and its messages go through Relay's outbox with the files
//! (review step 4, docs/relay-unified-delivery.md). A message to a device
//! that is away waits, survives a relaunch, and arrives once and in order
//! when the device is back. Two Relay devices on one machine.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::chat::{ChatCore, ChatMessageStatus};
use relay_core::{CodeSend, NearbyDevice, RelayCore};

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

fn chat(dir: &Path, name: &str, relay: &Arc<RelayCore>) -> Arc<ChatCore> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let core = loop {
        let folder = dir
            .join(format!("{name}-chat"))
            .to_string_lossy()
            .into_owned();
        match ChatCore::with_memory_credentials(folder) {
            Ok(core) => break core,
            Err(err) => {
                assert!(
                    Instant::now() < deadline,
                    "couldn't open {name}'s chat: {err}"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    Arc::clone(&core).use_relay(Arc::clone(relay));
    core
}

/// The bodies in `chat`'s conversation `id`, oldest first, and whether
/// each is sent.
fn bodies(chat: &ChatCore, id: &str) -> Vec<(String, ChatMessageStatus)> {
    block_on(chat.sync_messages(id.to_string()))
        .unwrap()
        .into_iter()
        .map(|m| (m.body, m.status))
        .collect()
}

#[test]
fn messages_to_a_device_that_is_away_survive_a_relaunch_and_arrive_once_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, _) = trusted_pair(dir.path());
    let laptop_chat = chat(dir.path(), "laptop", &laptop);
    // Signed out of any chat server, the device's conversation is listed.
    let list = block_on(laptop_chat.list_conversations()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "phone");
    let to_phone = list[0].id.clone();

    // The laptop saw the phone, then the phone went away.
    sees(&laptop, &phone);
    drop(phone);
    std::thread::sleep(Duration::from_millis(500));
    for text in ["one", "two"] {
        let sent = block_on(laptop_chat.send_message(to_phone.clone(), text.into())).unwrap();
        assert_eq!(sent.status, ChatMessageStatus::Sending, "{text} waits");
        assert!(sent.is_mine);
    }

    // They wait in Relay's outbox, not chat-core's.
    assert_eq!(laptop_chat.outbox_count(), 0);

    // Relay closes on the laptop and opens again: both still wait.
    drop(laptop_chat);
    drop(laptop);
    std::thread::sleep(Duration::from_millis(500));
    let laptop = device(dir.path(), "laptop");
    let laptop_chat = chat(dir.path(), "laptop", &laptop);
    assert_eq!(
        bodies(&laptop_chat, &to_phone),
        [
            ("one".to_string(), ChatMessageStatus::Sending),
            ("two".to_string(), ChatMessageStatus::Sending)
        ]
    );

    // The phone is back, and the laptop sees it; one answer is lost.
    let phone = device(dir.path(), "phone");
    let phone_chat = chat(dir.path(), "phone", &phone);
    let from_laptop = block_on(phone_chat.list_conversations()).unwrap()[0]
        .id
        .clone();
    laptop.lose_next_answers(1);
    sees(&laptop, &phone);
    eventually("both sent", || {
        bodies(&laptop_chat, &to_phone)
            .iter()
            .all(|(_, s)| *s == ChatMessageStatus::Sent)
    });
    let got = bodies(&phone_chat, &from_laptop);
    let texts: Vec<&str> = got.iter().map(|(b, _)| b.as_str()).collect();
    assert_eq!(texts, ["one", "two"], "once each, in order");
    let first = &block_on(phone_chat.sync_messages(from_laptop.clone())).unwrap()[0];
    assert!(!first.is_mine);
    assert_eq!(first.author, "laptop");

    // And back the other way, at once.
    sees(&phone, &laptop);
    let reply = block_on(phone_chat.send_message(from_laptop.clone(), "three".into())).unwrap();
    eventually("the reply delivered", || {
        bodies(&phone_chat, &from_laptop)
            .iter()
            .find(|(b, _)| b == "three")
            .is_some_and(|(_, s)| *s == ChatMessageStatus::Sent)
    });
    assert_eq!(reply.body, "three");
    eventually("the reply on the laptop", || {
        bodies(&laptop_chat, &to_phone).len() == 3
    });
    assert_eq!(bodies(&laptop_chat, &to_phone)[2].0, "three");
}

/// Messages and files to one device share Relay's outbox, so a device
/// that was away gets them in the order they were sent.
#[test]
fn messages_and_files_to_an_away_device_arrive_in_the_order_sent() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    sees(&laptop, &phone);
    drop(phone);
    std::thread::sleep(Duration::from_millis(500));

    let before = block_on(laptop.send_device_message(&phone_id, "before".into())).unwrap();
    assert_eq!(before.status, chat_core::MessageStatus::Queued);
    let file = dir.path().join("harbour.txt");
    std::fs::write(&file, b"the harbour at nine").unwrap();
    block_on(laptop.send_to_device(phone_id.clone(), vec![file.to_string_lossy().into_owned()]))
        .unwrap();
    block_on(laptop.send_device_message(&phone_id, "after".into())).unwrap();

    let phone = device(dir.path(), "phone");
    let laptop_id = phone.devices()[0].endpoint_id.clone();
    sees(&laptop, &phone);
    eventually("everything on the phone", || {
        phone.device_messages(&laptop_id).unwrap().len() == 2
            && phone
                .transfers()
                .iter()
                .any(|t| t.status == relay_core::TransferStatus::Done && !t.peer_id.is_empty())
    });
    let messages = phone.device_messages(&laptop_id).unwrap();
    let file_at = phone
        .transfers()
        .iter()
        .find(|t| matches!(&t.item, relay_core::Item::File { name, .. } if name == "harbour.txt"))
        .unwrap()
        .at_ms;
    assert_eq!(messages[0].body, "before");
    assert_eq!(messages[1].body, "after");
    assert!(messages[0].sent_at <= file_at && file_at <= messages[1].sent_at);
    eventually("both delivered on the laptop", || {
        laptop
            .device_messages(&phone_id)
            .unwrap()
            .iter()
            .all(|m| m.status == chat_core::MessageStatus::Sent)
    });
}

/// Revoking a device fails what waits for it; trusted again, a retry
/// delivers the message once.
#[test]
fn a_revoked_device_fails_a_waiting_message_and_a_retry_delivers_it_once() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    let laptop_chat = chat(dir.path(), "laptop", &laptop);
    let to_phone = block_on(laptop_chat.list_conversations()).unwrap()[0]
        .id
        .clone();
    sees(&laptop, &phone);
    drop(phone);
    std::thread::sleep(Duration::from_millis(500));

    let sent = block_on(laptop_chat.send_message(to_phone.clone(), "hello".into())).unwrap();
    block_on(laptop.revoke_device(phone_id.clone())).unwrap();
    eventually("the message failed", || {
        bodies(&laptop_chat, &to_phone) == [("hello".to_string(), ChatMessageStatus::Failed)]
    });

    block_on(laptop.trust_device(phone_id.clone())).unwrap();
    block_on(laptop_chat.retry_message(sent.id.clone())).unwrap();
    let phone = device(dir.path(), "phone");
    let laptop_id = phone.devices()[0].endpoint_id.clone();
    sees(&laptop, &phone);
    eventually("delivered", || {
        bodies(&laptop_chat, &to_phone) == [("hello".to_string(), ChatMessageStatus::Sent)]
    });
    let got = phone.device_messages(&laptop_id).unwrap();
    assert_eq!(got.len(), 1, "once");
    assert_eq!(got[0].body, "hello");
}
