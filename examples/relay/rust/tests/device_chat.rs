//! Device conversations: each trusted device is a conversation in Chats
//! on chat-core, and its messages go through Relay's outbox with the files
//! (review step 4, docs/relay-unified-delivery.md). A message to a device
//! that is away waits, survives a relaunch, and arrives once and in order
//! when the device is back. Two Relay devices on one machine.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_core::chat::{ChatCore, ChatMessageStatus};
use relay_core::{CodeSend, NearbyDevice, RelayCore, TransferStatus};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    forge_runtime::block_on(future)
}

/// Opens a device, waiting while a closing one still holds its folder.
fn device(dir: &Path, name: &str) -> Arc<RelayCore> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let core = loop {
        match RelayCore::with_default_servers(
            dir.join(name).to_string_lossy().into_owned(),
            name.into(),
            relay_core::RelayServers::default(),
        ) {
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
    // The picked file can go: its bytes wait in the store.
    std::fs::remove_file(&file).unwrap();

    let phone = device(dir.path(), "phone");
    let laptop_id = phone.devices()[0].endpoint_id.clone();
    sees(&laptop, &phone);
    eventually("everything on the phone", || {
        phone.device_messages(&laptop_id).unwrap().len() == 3
    });
    let messages = phone.device_messages(&laptop_id).unwrap();
    assert_eq!(messages[0].body, "before");
    assert_eq!(messages[1].attachments[0].name, "harbour.txt");
    // One attachment identity on both sides, though the sent file is gone.
    let sent = laptop.device_messages(&phone_id).unwrap();
    assert_eq!(sent[1].attachments, messages[1].attachments);
    assert_eq!(messages[2].body, "after");
    assert!(messages[0].sent_at < messages[1].sent_at && messages[1].sent_at < messages[2].sent_at);
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

/// Files sent to a trusted device are a message with one attachment in
/// both conversations, with the right size, and open from where they are:
/// the file that was sent, the file and folder that were saved.
#[test]
fn files_to_a_device_are_attachments_in_both_conversations() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    let laptop_id = phone.devices()[0].endpoint_id.clone();
    sees(&laptop, &phone);
    let file = dir.path().join("harbour.jpg");
    std::fs::write(&file, b"not really a photo").unwrap();
    let folder = dir.path().join("trip");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), b"one").unwrap();
    std::fs::write(folder.join("b.txt"), b"three").unwrap();
    for path in [&file, &folder] {
        block_on(
            laptop.send_to_device(phone_id.clone(), vec![path.to_string_lossy().into_owned()]),
        )
        .unwrap();
    }

    eventually("both on the phone", || {
        phone.device_messages(&laptop_id).unwrap().len() == 2
    });
    eventually("both delivered on the laptop", || {
        let sent = laptop.device_messages(&phone_id).unwrap();
        sent.len() == 2
            && sent
                .iter()
                .all(|m| m.status == chat_core::MessageStatus::Sent)
    });
    let sent = laptop.device_messages(&phone_id).unwrap();
    let got = phone.device_messages(&laptop_id).unwrap();
    for (ours, theirs) in sent.iter().zip(&got) {
        assert_eq!(ours.id, theirs.id, "one id on both sides");
        assert_eq!(ours.attachments, theirs.attachments);
        assert_eq!(theirs.author_id, laptop_id);
    }
    let (photo, trip) = (&got[0].attachments[0], &got[1].attachments[0]);
    assert_eq!(
        (photo.name.as_str(), photo.mime.as_str(), photo.size),
        ("harbour.jpg", "image/jpeg", 18)
    );
    assert_eq!(
        (trip.name.as_str(), trip.mime.as_str(), trip.size),
        ("trip", "inode/directory", 8)
    );

    let saved = phone.device_file(&laptop_id, &photo.id).unwrap();
    assert_eq!(std::fs::read(&saved).unwrap(), b"not really a photo");
    assert!(Path::new(&phone.device_file(&laptop_id, &trip.id).unwrap())
        .join("b.txt")
        .is_file());
    // The laptop kept a copy of the photo it sent: it opens after the
    // picked file is gone, from Chats too.
    std::fs::remove_file(&file).unwrap();
    let kept = laptop.device_file(&phone_id, &photo.id).unwrap();
    assert_eq!(std::fs::read(&kept).unwrap(), b"not really a photo");
    let laptop_chat = chat(dir.path(), "laptop", &laptop);
    assert_eq!(laptop_chat.attachment_file(photo.id.clone()), Some(kept));
}

/// A note from a device on an older build is a message in its
/// conversation, and a retry of it adds nothing.
#[test]
fn a_note_from_an_older_build_is_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    let laptop_id = phone.devices()[0].endpoint_id.clone();
    sees(&laptop, &phone);
    block_on(laptop.send_note_as_older_build(phone_id, "from the old build".into())).unwrap();
    eventually("the note as a message", || {
        phone.device_messages(&laptop_id).unwrap().len() == 1
    });
    let got = phone.device_messages(&laptop_id).unwrap();
    assert_eq!(got[0].body, "from the old build");
    assert!(got[0].id.starts_with("note-"));
    assert!(!phone
        .transfers()
        .iter()
        .any(|t| matches!(t.item, relay_core::Item::Note { .. })));
}

/// A file whose bytes were damaged in the sender's store is refused: the
/// receiver checks every chunk, saves nothing, and both sides say why.
#[test]
fn a_damaged_file_fails_verification_and_is_not_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, phone_id) = trusted_pair(dir.path());
    sees(&laptop, &phone);
    drop(phone);
    std::thread::sleep(Duration::from_millis(500));
    let file = dir.path().join("big.bin");
    let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&file, &bytes).unwrap();
    block_on(laptop.send_to_device(phone_id.clone(), vec![file.to_string_lossy().into_owned()]))
        .unwrap();
    drop(laptop);
    std::thread::sleep(Duration::from_millis(500));

    // Damage the bytes the laptop keeps to send.
    let mut damaged = 0;
    for entry in walk(&dir.path().join("laptop")) {
        if entry.extension().is_some_and(|e| e == "data") {
            let mut data = std::fs::read(&entry).unwrap();
            if data.len() >= 200_000 {
                data[100_000] ^= 0xff;
                std::fs::write(&entry, data).unwrap();
                damaged += 1;
            }
        }
    }
    assert_eq!(damaged, 1, "the blob on disk");

    let laptop = device(dir.path(), "laptop");
    let phone = device(dir.path(), "phone");
    let laptop_id = phone.devices()[0].endpoint_id.clone();
    sees(&laptop, &phone);
    eventually("the send ends on the laptop", || {
        laptop.device_messages(&phone_id).unwrap()[0].status != chat_core::MessageStatus::Queued
    });
    let sent = laptop
        .transfers()
        .into_iter()
        .find(|t| t.peer_id == phone_id)
        .unwrap();
    let got = phone
        .transfers()
        .into_iter()
        .find(|t| t.peer_id == laptop_id)
        .unwrap();
    let failed = |status: TransferStatus| match status {
        TransferStatus::Failed { reason } => reason,
        other => panic!("not failed: {other:?}"),
    };
    assert!(failed(sent.status).contains("verification failed"));
    assert!(failed(got.status).starts_with("Verification failed"));
    assert_eq!(
        laptop.device_messages(&phone_id).unwrap()[0].status,
        chat_core::MessageStatus::Failed
    );
    // Nothing was saved, and nothing shows in the phone's conversation.
    assert!(phone.device_messages(&laptop_id).unwrap().is_empty());
    let received = walk(&dir.path().join("phone").join("Received"));
    assert!(
        !received.iter().any(|p| p.ends_with("big.bin")),
        "{received:?}"
    );
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}
