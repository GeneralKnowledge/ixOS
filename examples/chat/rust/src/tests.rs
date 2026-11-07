//! The core against the real chat server, in process, over sockets.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use forge_runtime::block_on;

use super::*;
use crate::native::NativeServices;

const WAIT: Duration = Duration::from_secs(10);

/// A chat server on its own runtime; dropping it kills it.
struct Server {
    runtime: Option<tokio::runtime::Runtime>,
    url: String,
}

impl Server {
    fn start(state: chat_server::AppState) -> Self {
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
        let addr: SocketAddr = listener.local_addr().unwrap();
        runtime.spawn(async move { axum::serve(listener, chat_server::router(state)).await });
        Self {
            runtime: Some(runtime),
            url: format!("http://{addr}"),
        }
    }

    fn in_memory() -> Self {
        Self::start(chat_server::AppState::new(Duration::from_millis(100)))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

fn next_matching(events: &ChatEvents, what: &str, pred: impl Fn(&ChatEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        let event = block_on(events.next()).expect("event stream ended");
        if pred(&event) {
            return;
        }
        seen.push(event);
    }
    panic!("timed out waiting for {what}; saw {seen:#?}");
}

fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A core whose account is kept by the Keychain adapter over the
/// forge-services fakes, so the real Keychain code path runs.
fn keychain_core(dir: &std::path::Path) -> (ChatCore, Arc<dyn Credentials>) {
    let (services, _fakes) = NativeServices::for_tests(dir.join("services"));
    let credentials: Arc<dyn Credentials> = Arc::new(KeychainCredentials::new(services));
    let core = ChatCore::open(dir.join("chats"), credentials.clone()).unwrap();
    (core, credentials)
}

#[test]
fn sign_up_keeps_the_account_in_the_keychain_not_in_files() {
    let server = Server::in_memory();
    let dir = tempfile::tempdir().unwrap();
    let (core, credentials) = keychain_core(dir.path());
    assert_eq!(core.account(), None);
    assert!(matches!(
        block_on(core.list_conversations()),
        Err(ChatError::NotSignedIn)
    ));
    let events = core.events().unwrap();

    let account = block_on(core.sign_up(format!("{}/", server.url), "  Reuben ".into())).unwrap();
    assert_eq!(account.display_name, "Reuben");
    assert_eq!(account.server_url, server.url);
    next_matching(
        &events,
        "AccountChanged",
        |e| matches!(e, ChatEvent::AccountChanged { account: Some(a) } if a.user_id == account.user_id),
    );
    let stored = credentials.load().unwrap().unwrap();
    assert_eq!(stored.public(), account);
    assert_eq!(stored.token.len(), 64);
    assert_eq!(block_on(core.check_account()).unwrap(), account);

    // The token is in no file the app writes.
    for entry in walk(&dir.path().join("chats")) {
        let text = String::from_utf8_lossy(&std::fs::read(&entry).unwrap()).into_owned();
        assert!(!text.contains(&stored.token), "{entry:?} holds the token");
    }

    // A relaunch is signed in at once, without the network.
    core.shutdown();
    drop(server);
    let again = ChatCore::open(dir.path().join("chats"), credentials).unwrap();
    assert_eq!(again.account(), Some(account));
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn two_accounts_chat_and_own_messages_are_marked_by_account() {
    let server = Server::in_memory();
    let dir = tempfile::tempdir().unwrap();
    let ana =
        ChatCore::with_memory_credentials(dir.path().join("ana").to_string_lossy().into()).unwrap();
    let ben =
        ChatCore::with_memory_credentials(dir.path().join("ben").to_string_lossy().into()).unwrap();
    // Same display name: only the account id tells them apart.
    block_on(ana.sign_up(server.url.clone(), "Sam".into())).unwrap();
    block_on(ben.sign_up(server.url.clone(), "Sam".into())).unwrap();

    let ben_events = ben.events().unwrap();
    let rooms = block_on(ben.list_conversations()).unwrap();
    assert_eq!(rooms[0].id, "general");
    ben.watch("general".into()).unwrap();
    next_matching(&ben_events, "Ben connected", |e| {
        matches!(e, ChatEvent::StreamConnected { .. })
    });

    block_on(ana.list_conversations()).unwrap();
    let sent = block_on(ana.send_message("general".into(), "hello Ben".into())).unwrap();
    assert!(sent.is_mine);
    assert_eq!(sent.status, ChatMessageStatus::Sent);

    next_matching(
        &ben_events,
        "a notification",
        |e| matches!(e, ChatEvent::Notify { message } if message.id == sent.id && !message.is_mine),
    );
    next_matching(
        &ben_events,
        "an unread count",
        |e| matches!(e, ChatEvent::ConversationUpdated { conversation } if conversation.unread_count == 1),
    );
    let seen_by_ben = ben.cached_messages("general".into());
    assert_eq!(seen_by_ben.len(), 1);
    assert!(!seen_by_ben[0].is_mine);
    assert!(ana.cached_messages("general".into())[0].is_mine);
}

#[test]
fn background_stops_streams_and_resume_catches_up() {
    let server = Server::in_memory();
    let dir = tempfile::tempdir().unwrap();
    let me =
        ChatCore::with_memory_credentials(dir.path().join("me").to_string_lossy().into()).unwrap();
    let other =
        ChatCore::with_memory_credentials(dir.path().join("other").to_string_lossy().into())
            .unwrap();
    block_on(me.sign_up(server.url.clone(), "Me".into())).unwrap();
    block_on(other.sign_up(server.url.clone(), "Other".into())).unwrap();
    let events = me.events().unwrap();
    me.set_phase(AppPhase::Active);
    block_on(me.list_conversations()).unwrap();
    me.subscribe("general".into()).unwrap();
    next_matching(&events, "connected", |e| {
        matches!(e, ChatEvent::StreamConnected { .. })
    });

    me.set_phase(AppPhase::Background);
    wait_until("streams stop", || me.live_streams() == 0);
    next_matching(&events, "PhaseChanged(Background)", |e| {
        matches!(
            e,
            ChatEvent::PhaseChanged {
                phase: AppPhase::Background,
                ..
            }
        )
    });

    block_on(other.list_conversations()).unwrap();
    let missed = block_on(other.send_message("general".into(), "while away".into())).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(me.cached_messages("general".into()).is_empty());

    me.set_phase(AppPhase::Active);
    next_matching(&events, "resumed", |e| {
        matches!(
            e,
            ChatEvent::PhaseChanged {
                phase: AppPhase::Active,
                resumed: true
            }
        )
    });
    next_matching(
        &events,
        "the missed message",
        |e| matches!(e, ChatEvent::MessageReceived { message } if message.id == missed.id),
    );
    assert_eq!(me.live_streams(), 1);

    me.set_phase(AppPhase::Terminated);
    assert!(matches!(
        block_on(me.list_conversations()),
        Err(ChatError::ShutDown)
    ));
}

#[test]
fn a_server_that_forgot_the_account_is_reported_not_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let core = ChatCore::with_memory_credentials(dir.path().to_string_lossy().into()).unwrap();
    let first = Server::in_memory();
    block_on(core.sign_up(first.url.clone(), "Me".into())).unwrap();
    // Same address, fresh data: as if the server's data file was deleted.
    let url = first.url.clone();
    drop(first);
    let port: u16 = url.rsplit(':').next().unwrap().parse().unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = match runtime.block_on(tokio::net::TcpListener::bind(("127.0.0.1", port))) {
        Ok(listener) => listener,
        // The port was taken in between; nothing to test on this run.
        Err(_) => return,
    };
    runtime.spawn(async move {
        axum::serve(
            listener,
            chat_server::router(chat_server::AppState::new(Duration::from_millis(100))),
        )
        .await
    });

    let events = core.events().unwrap();
    assert!(matches!(
        block_on(core.list_conversations()),
        Err(ChatError::Unauthorized { .. })
    ));
    next_matching(&events, "AccountRejected", |e| {
        matches!(e, ChatEvent::AccountRejected { .. })
    });
    assert!(matches!(
        block_on(core.check_account()),
        Err(ChatError::Unauthorized { .. })
    ));
    // Still signed in until the user decides.
    assert!(core.account().is_some());
    runtime.shutdown_timeout(Duration::from_secs(1));
}

#[test]
fn sign_out_forgets_the_account_and_its_chats() {
    let server = Server::in_memory();
    let dir = tempfile::tempdir().unwrap();
    let (core, credentials) = keychain_core(dir.path());
    let account = block_on(core.sign_up(server.url.clone(), "Me".into())).unwrap();
    block_on(core.list_conversations()).unwrap();
    block_on(core.send_message("general".into(), "kept until sign-out".into())).unwrap();
    let chats = dir.path().join("chats/accounts");
    assert!(walk(&chats)
        .iter()
        .any(|p| p.to_string_lossy().contains(&account.user_id)));

    let events = core.events().unwrap();
    core.sign_out().unwrap();
    next_matching(&events, "AccountChanged(None)", |e| {
        matches!(e, ChatEvent::AccountChanged { account: None })
    });
    assert_eq!(core.account(), None);
    assert_eq!(credentials.load().unwrap(), None);
    assert!(walk(&chats).is_empty());
    assert!(matches!(
        block_on(core.send_message("general".into(), "x".into())),
        Err(ChatError::NotSignedIn)
    ));
    core.sign_out().unwrap();
}

#[test]
fn bad_input_is_rejected_before_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let core = ChatCore::with_memory_credentials(dir.path().to_string_lossy().into()).unwrap();
    assert!(matches!(
        block_on(core.sign_up("localhost:8787".into(), "Me".into())),
        Err(ChatError::InvalidInput { .. })
    ));
    assert!(matches!(
        block_on(core.sign_up("http://127.0.0.1:9".into(), "  ".into())),
        Err(ChatError::InvalidInput { .. })
    ));
    assert!(matches!(
        ChatCore::with_memory_credentials(String::new()),
        Err(ChatError::InvalidInput { .. })
    ));
}

#[test]
fn without_installed_services_the_keychain_constructor_says_why() {
    NativeServices::uninstall();
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        ChatCore::new(dir.path().to_string_lossy().into()),
        Err(ChatError::UnsupportedOnHost { .. })
    ));
}

#[test]
fn read_receipts_new_rooms_and_mute_through_the_ffi() {
    let server = Server::in_memory();
    let dir = tempfile::tempdir().unwrap();
    let me =
        ChatCore::with_memory_credentials(dir.path().join("me").to_string_lossy().into()).unwrap();
    let other =
        ChatCore::with_memory_credentials(dir.path().join("other").to_string_lossy().into())
            .unwrap();
    block_on(me.sign_up(server.url.clone(), "Me".into())).unwrap();
    block_on(other.sign_up(server.url.clone(), "Other".into())).unwrap();
    let events = me.events().unwrap();

    // A new room, created here and found by the other account.
    let room = block_on(me.create_conversation("  Plans ".into())).unwrap();
    assert_eq!(room.title, "Plans");
    assert!(!room.shows_unread && room.last_message.is_none());
    assert!(block_on(other.list_conversations())
        .unwrap()
        .iter()
        .any(|c| c.id == room.id));
    assert!(matches!(
        block_on(me.create_conversation(" ".into())),
        Err(ChatError::InvalidInput { .. })
    ));

    // Our message is Sent until the other account opens the room.
    me.watch(room.id.clone()).unwrap();
    next_matching(&events, "connected", |e| {
        matches!(e, ChatEvent::StreamConnected { .. })
    });
    let sent = block_on(me.send_message(room.id.clone(), "see you at 8".into())).unwrap();
    let mine = me
        .cached_conversations()
        .into_iter()
        .find(|c| c.id == room.id)
        .unwrap();
    assert_eq!(mine.last_message.as_ref().unwrap().body, "see you at 8");
    assert!(mine.last_message.as_ref().unwrap().is_mine);
    assert_eq!(chat_delivery(sent.clone(), Some(mine)), ChatDelivery::Sent);

    block_on(other.list_conversations()).unwrap();
    let theirs = other
        .cached_conversations()
        .into_iter()
        .find(|c| c.id == room.id)
        .unwrap();
    assert!(theirs.shows_unread);
    assert_eq!(theirs.unread_count, 1);
    assert!(!theirs.last_message.unwrap().is_mine);
    block_on(other.mark_read(room.id.clone())).unwrap();

    next_matching(&events, "read by the other account", |e| {
        matches!(e, ChatEvent::ConversationUpdated { conversation }
            if chat_delivery(sent.clone(), Some(conversation.clone())) == ChatDelivery::Read)
    });

    // Mute and marked-unread are this device's.
    me.set_muted(room.id.clone(), true).unwrap();
    me.set_marked_unread(room.id.clone(), true).unwrap();
    let mine = me
        .cached_conversations()
        .into_iter()
        .find(|c| c.id == room.id)
        .unwrap();
    assert!(mine.muted && mine.marked_unread && mine.shows_unread);
    block_on(me.mark_read(room.id.clone())).unwrap();
    assert!(!me.cached_conversations().iter().any(|c| c.marked_unread));
    assert!(matches!(
        me.set_muted("no-such-room".into(), true),
        Err(ChatError::NotFound { .. })
    ));
}
