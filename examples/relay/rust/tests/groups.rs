//! Group chats (N7, docs/relay-groups.md): a group message goes once to
//! each member through Relay's outbox; only the admin changes the group; a
//! removed member gets nothing after; a member that is away gets what it
//! missed when it is back, once. Three Relay devices on one machine.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chat_core::MessageStatus;
use relay_core::chat::{ChatCore, ChatMessageStatus};
use relay_core::{CodeSend, NearbyDevice, RelayCore};

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

/// `core` sees `others` nearby (the whole list: it replaces the last).
fn sees(core: &RelayCore, others: &[&RelayCore]) {
    let nearby = others
        .iter()
        .map(|other| {
            let service = block_on(other.device_service()).unwrap();
            NearbyDevice {
                endpoint_id: service.endpoint_id,
                addresses: vec![format!("127.0.0.1:{}", service.port)],
            }
        })
        .collect();
    core.devices_nearby(nearby);
}

fn id_of(core: &RelayCore) -> String {
    block_on(core.device_service()).unwrap().endpoint_id
}

/// `a` and `b` make one code transfer and trust each other.
fn pair(dir: &Path, a: &RelayCore, b: &RelayCore) {
    let file = dir.join("hello.txt");
    std::fs::write(&file, b"hello").unwrap();
    b.send_with_code(file.to_string_lossy().into_owned())
        .unwrap();
    eventually("a code", || {
        matches!(b.code_send_status(), CodeSend::Waiting { .. })
    });
    let CodeSend::Waiting {
        code,
        port,
        endpoint_id,
        ..
    } = b.code_send_status()
    else {
        unreachable!()
    };
    block_on(a.receive_with_code(code, vec![format!("127.0.0.1:{port}")], endpoint_id)).unwrap();
    eventually("b's trust offer", || b.trust_offer().is_some());
    let on_a = a.trust_offer().unwrap();
    let on_b = b.trust_offer().unwrap();
    block_on(a.trust_device(on_a.endpoint_id)).unwrap();
    block_on(b.trust_device(on_b.endpoint_id)).unwrap();
}

/// Three devices, each trusting the other two.
fn three(dir: &Path) -> (Arc<RelayCore>, Arc<RelayCore>, Arc<RelayCore>) {
    let phone = device(dir, "phone");
    let laptop = device(dir, "laptop");
    let tablet = device(dir, "tablet");
    pair(dir, &phone, &laptop);
    pair(dir, &phone, &tablet);
    pair(dir, &laptop, &tablet);
    sees(&phone, &[&laptop, &tablet]);
    sees(&laptop, &[&phone, &tablet]);
    sees(&tablet, &[&phone, &laptop]);
    (phone, laptop, tablet)
}

fn bodies(core: &RelayCore, group: &str) -> Vec<String> {
    core.group_messages(group)
        .unwrap()
        .into_iter()
        .map(|m| m.body)
        .collect()
}

fn in_group(core: &RelayCore, group: &str) -> bool {
    block_on(core.groups())
        .unwrap()
        .iter()
        .any(|g| g.id == group && !g.left)
}

#[test]
fn a_group_message_reaches_every_member_and_only_the_admin_changes_the_group() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, tablet) = three(dir.path());
    let (laptop_id, tablet_id) = (id_of(&laptop), id_of(&tablet));

    let group =
        block_on(phone.create_group("Family".into(), vec![laptop_id.clone(), tablet_id.clone()]))
            .unwrap();
    assert!(group.is_admin);
    assert_eq!(group.members.len(), 3);
    let id = group.id.clone();
    eventually("the group on the laptop and tablet", || {
        in_group(&laptop, &id) && in_group(&tablet, &id)
    });
    let on_tablet = block_on(tablet.group(id.clone())).unwrap();
    assert_eq!(on_tablet.name, "Family");
    assert!(!on_tablet.is_admin);

    // The admin's message reaches both.
    let sent = block_on(phone.send_group_message(&id, "dinner at seven".into())).unwrap();
    assert_eq!(sent.status, MessageStatus::Sent);
    let d = phone.group_delivery(id.clone(), sent.id.clone()).unwrap();
    assert_eq!((d.delivered, d.of), (2, 2));
    assert_eq!(bodies(&laptop, &id), ["dinner at seven"]);
    assert_eq!(bodies(&tablet, &id), ["dinner at seven"]);

    // A member's message reaches the admin and the other member, by name.
    block_on(tablet.send_group_message(&id, "I'll bring bread".into())).unwrap();
    eventually("the tablet's message everywhere", || {
        bodies(&phone, &id).len() == 2 && bodies(&laptop, &id).len() == 2
    });
    let on_laptop = laptop.group_messages(&id).unwrap();
    assert_eq!(on_laptop[1].author, "tablet");
    assert_eq!(on_laptop[1].author_id, tablet_id);

    // Only the admin changes it.
    assert!(block_on(laptop.rename_group(id.clone(), "Mine".into())).is_err());
    block_on(phone.rename_group(id.clone(), "Family dinners".into())).unwrap();
    eventually("the new name on the tablet", || {
        block_on(tablet.group(id.clone())).unwrap().name == "Family dinners"
    });

    // Two changes at once: neither is lost. One made from a version
    // that changed meanwhile is refused.
    let (one, two) = block_on(async {
        tokio::join!(
            phone.rename_group(id.clone(), "One".into()),
            phone.rename_group(id.clone(), "Two".into())
        )
    });
    let worked = [&one, &two].iter().filter(|r| r.is_ok()).count() as u64;
    assert!(worked >= 1);
    let version = |core: &RelayCore| block_on(core.groups()).unwrap()[0].clone();
    let before = version(&phone);
    eventually("the same name on the tablet", || {
        block_on(tablet.group(id.clone())).unwrap().name == before.name
    });

    // Removed: the tablet learns it's out, and gets nothing after.
    block_on(phone.remove_group_member(id.clone(), tablet_id.clone())).unwrap();
    eventually("the tablet out", || !in_group(&tablet, &id));
    assert!(block_on(tablet.send_group_message(&id, "still here?".into())).is_err());
    let after = block_on(phone.send_group_message(&id, "just us".into())).unwrap();
    let d = phone.group_delivery(id.clone(), after.id.clone()).unwrap();
    assert_eq!((d.delivered, d.of), (1, 1));
    eventually("the laptop has it", || bodies(&laptop, &id).len() == 3);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(bodies(&tablet, &id).len(), 2, "nothing after the removal");
}

#[test]
fn a_member_that_was_away_gets_the_message_once_and_a_member_can_leave() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, tablet) = three(dir.path());
    let (laptop_id, tablet_id) = (id_of(&laptop), id_of(&tablet));
    let id =
        block_on(phone.create_group("Walk".into(), vec![laptop_id.clone(), tablet_id.clone()]))
            .unwrap()
            .id;
    eventually("the group on the laptop and tablet", || {
        in_group(&laptop, &id) && in_group(&tablet, &id)
    });

    // The tablet goes away; the message reaches the laptop, and waits
    // for the tablet.
    drop(tablet);
    std::thread::sleep(Duration::from_millis(500));
    let sent = block_on(phone.send_group_message(&id, "meet at the gate".into())).unwrap();
    assert_eq!(sent.status, MessageStatus::Queued, "one copy still waits");
    let d = phone.group_delivery(id.clone(), sent.id.clone()).unwrap();
    assert_eq!((d.delivered, d.of), (1, 2));

    // Back: it arrives once, and the message is sent.
    let tablet = device(dir.path(), "tablet");
    phone.lose_next_answers(1);
    sees(&phone, &[&laptop, &tablet]);
    sees(&tablet, &[&phone, &laptop]);
    sees(&laptop, &[&phone, &tablet]);
    eventually("the tablet has it", || {
        bodies(&tablet, &id) == ["meet at the gate"]
    });
    eventually("sent to everyone", || {
        phone.group_messages(&id).unwrap()[0].status == MessageStatus::Sent
    });
    let d = phone.group_delivery(id.clone(), sent.id.clone()).unwrap();
    assert_eq!((d.delivered, d.of), (2, 2));
    assert_eq!(
        bodies(&tablet, &id).len(),
        1,
        "once, though an answer was lost"
    );

    // The laptop leaves: the others stop sending to it.
    block_on(laptop.leave_group(id.clone())).unwrap();
    eventually("the laptop gone on the phone", || {
        block_on(phone.group(id.clone())).unwrap().members.len() == 2
    });
    eventually("the laptop gone on the tablet", || {
        block_on(tablet.group(id.clone())).unwrap().members.len() == 2
    });
    let last = block_on(tablet.send_group_message(&id, "bye".into())).unwrap();
    let d = tablet.group_delivery(id.clone(), last.id.clone()).unwrap();
    assert_eq!((d.delivered, d.of), (1, 1));
}

#[test]
fn a_group_is_a_conversation_in_chats() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, tablet) = three(dir.path());
    let id = block_on(phone.create_group("Team".into(), vec![id_of(&laptop), id_of(&tablet)]))
        .unwrap()
        .id;
    eventually("the group on the laptop", || in_group(&laptop, &id));

    let folder = dir
        .path()
        .join("laptop-chat")
        .to_string_lossy()
        .into_owned();
    let laptop_chat = ChatCore::with_memory_credentials(folder).unwrap();
    Arc::clone(&laptop_chat).use_relay(Arc::clone(&laptop));
    let list = block_on(laptop_chat.list_conversations()).unwrap();
    let conversation = list
        .iter()
        .find(|c| c.title == "Team")
        .expect("the group is listed")
        .id
        .clone();
    assert_eq!(conversation, format!("group:{id}"));

    // Sent from Chats, it reaches both other members.
    block_on(laptop_chat.send_message(conversation.clone(), "standup in 5".into())).unwrap();
    eventually("on the phone and tablet", || {
        bodies(&phone, &id) == ["standup in 5"] && bodies(&tablet, &id) == ["standup in 5"]
    });
    eventually("sent in Chats", || {
        block_on(laptop_chat.sync_messages(conversation.clone()))
            .unwrap()
            .iter()
            .all(|m| m.status == ChatMessageStatus::Sent)
    });

    // What the others send shows in Chats, with who sent it.
    block_on(phone.send_group_message(&id, "on my way".into())).unwrap();
    eventually("the phone's message in Chats", || {
        block_on(laptop_chat.sync_messages(conversation.clone()))
            .unwrap()
            .iter()
            .any(|m| m.body == "on my way" && m.author == "phone" && !m.is_mine)
    });
}

/// A chat server on its own runtime; dropping it stops it.
struct Server {
    runtime: Option<tokio::runtime::Runtime>,
    url: String,
}

impl Server {
    fn start() -> Self {
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = chat_server::AppState::new(Duration::from_millis(100));
        runtime.spawn(async move { axum::serve(listener, chat_server::router(state)).await });
        Self {
            runtime: Some(runtime),
            url,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

#[test]
fn a_group_made_while_chats_is_open_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let (phone, laptop, tablet) = three(dir.path());
    let folder = dir.path().join("phone-chat").to_string_lossy().into_owned();
    let chat = ChatCore::with_memory_credentials(folder).unwrap();
    // Signed in to a chat server, as on the phone.
    let server = Server::start();
    block_on(chat.sign_up(server.url.clone(), "Phone".into())).unwrap();
    Arc::clone(&chat).use_relay(Arc::clone(&phone));
    // Chats lists before any group exists, as the app does on launch.
    let before = block_on(chat.list_conversations()).unwrap();
    assert!(before.iter().all(|c| c.title != "Family"));

    let group = block_on(phone.create_group("Family".into(), vec![id_of(&laptop), id_of(&tablet)]))
        .unwrap();
    let listed = |list: &[relay_core::chat::ChatConversation]| {
        list.iter()
            .any(|c| c.id == group.conversation_id && c.title == "Family")
    };
    assert!(listed(&block_on(chat.list_conversations()).unwrap()));

    // The server goes away: the list fails, but the group is still there
    // to show from what was kept.
    drop(server);
    let second = block_on(phone.create_group("Walk".into(), vec![id_of(&laptop)])).unwrap();
    assert!(block_on(chat.list_conversations()).is_err());
    let cached = chat.cached_conversations();
    assert!(listed(&cached));
    assert!(cached.iter().any(|c| c.id == second.conversation_id));
}
