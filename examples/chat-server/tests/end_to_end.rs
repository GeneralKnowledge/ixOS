//! chat-core's real HTTP + SSE client against the real chat server, in
//! process: the demo interaction from docs/chat-demo.md over sockets.

use std::net::SocketAddr;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chat_core::{ChatClient, ChatConfig, ChatError, ChatEvent, ChatStore, HttpRepo, MessageStatus};
use chat_server::{router, AppState};

const WAIT: Duration = Duration::from_secs(10);

/// A server on its own runtime. Dropping it kills the server and every
/// open connection, like Ctrl-C.
struct Server {
    runtime: Option<tokio::runtime::Runtime>,
}

impl Server {
    fn start(state: AppState, addr: SocketAddr) -> (Self, SocketAddr) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind(addr))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        runtime.spawn(async move { axum::serve(listener, router(state)).await });
        (
            Self {
                runtime: Some(runtime),
            },
            addr,
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

fn no_proxy() {
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
}

fn client(addr: SocketAddr) -> (ChatClient, Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    let repo = HttpRepo::with_ureq(format!("http://{addr}")).timeout(Duration::from_secs(2));
    let config = ChatConfig {
        reconnect_initial: Duration::from_millis(20),
        reconnect_max: Duration::from_millis(100),
        ..ChatConfig::default()
    };
    let client = ChatClient::open(Arc::new(repo), ChatStore::in_memory(), config, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (client, rx)
}

fn wait_for(rx: &Receiver<ChatEvent>, what: &str, pred: impl Fn(&ChatEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(event) if pred(&event) => return,
            Ok(event) => seen.push(event),
            Err(_) => panic!("timed out waiting for {what}; saw {seen:#?}"),
        }
    }
}

fn connected(e: &ChatEvent) -> bool {
    matches!(e, ChatEvent::StreamConnected { conversation_id } if conversation_id == "general")
}

/// What `curl -X POST .../rooms/general/messages` does.
fn post_external(addr: SocketAddr, body: &str) {
    post_external_to(addr, "general", body);
}

fn post_external_to(addr: SocketAddr, room: &str, body: &str) {
    raw_http::post(
        addr,
        &format!("/rooms/{room}/messages"),
        &format!(r#"{{"author":"curl","body":"{body}"}}"#),
    );
}

/// A tiny raw-socket HTTP POST, so the test needs no extra HTTP client.
mod raw_http {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};

    pub fn post(addr: SocketAddr, path: &str, json: &str) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 201"), "{response}");
        response
    }
}

#[test]
fn rooms_messages_and_errors_over_http() {
    no_proxy();
    let (_server, addr) = Server::start(
        AppState::new(Duration::from_millis(100)).allow_anonymous(true),
        "127.0.0.1:0".parse().unwrap(),
    );
    let (client, _rx) = client(addr);

    let rooms = client.list_conversations().unwrap();
    let ids: Vec<_> = rooms.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["general", "random", "forge"]);

    let sent = client.send_message("general", "hello").unwrap();
    assert_eq!(sent.status, MessageStatus::Sent);
    // Retrying a sent id is idempotent on the server too.
    let again = chat_core::ChatRepo::post_message(
        &HttpRepo::with_ureq(format!("http://{addr}")),
        &chat_core::OutgoingMessage {
            id: sent.id.clone(),
            conversation_id: "general".into(),
            author: "me".into(),
            author_id: String::new(),
            body: "different".into(),
        },
    )
    .unwrap();
    assert_eq!(
        (again.body.as_str(), again.sent_at),
        ("hello", sent.sent_at)
    );

    post_external(addr, "one");
    post_external(addr, "two");
    let all = client.load_messages("general", None).unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.windows(2).all(|w| w[0].sent_at < w[1].sent_at));
    let newer = client
        .load_messages("general", Some(all[1].sent_at))
        .unwrap();
    assert_eq!(newer.len(), 3, "load returns the full stored list");
    assert_eq!(client.last_synced("general"), Some(all[2].sent_at));

    assert!(matches!(
        client.load_messages("nope", None),
        Err(ChatError::NotFound { .. })
    ));
    assert!(matches!(
        client.send_message("nope", "x"),
        Err(ChatError::NotFound { .. })
    ));
}

#[test]
fn killer_demo_interaction_over_real_sockets() {
    no_proxy();
    let state = AppState::new(Duration::from_millis(100)).allow_anonymous(true);
    let (server, addr) = Server::start(state.clone(), "127.0.0.1:0".parse().unwrap());
    let (client, rx) = client(addr);
    client.list_conversations().unwrap();

    // 1. Open a chat and send.
    client.subscribe("general").unwrap();
    wait_for(&rx, "connect", connected);
    client.sync_messages("general").unwrap();
    assert_eq!(
        client.send_message("general", "first").unwrap().status,
        MessageStatus::Sent
    );

    // 2. Back to the list, then into the chat again.
    client.unsubscribe("general").unwrap();
    client.list_conversations().unwrap();
    client.subscribe("general").unwrap();
    wait_for(&rx, "reconnect", connected);

    // 3. Kill the server.
    drop(server);
    wait_for(&rx, "stream drop", |e| {
        matches!(
            e,
            ChatEvent::StreamDisconnected {
                will_retry: true,
                ..
            }
        )
    });

    // 4. Send: typed network error, Failed bubble.
    let err = client.send_message("general", "second").unwrap_err();
    assert!(err.is_network(), "{err:?}");
    let failed = client
        .cached_messages("general")
        .into_iter()
        .find(|m| m.status == MessageStatus::Failed)
        .unwrap();

    // 5. Restart on the same port (same in-memory state), retry: Sent.
    let (_server, _) = Server::start(state, addr);
    let retried = client.retry_message(&failed.id).unwrap();
    assert_eq!(retried.status, MessageStatus::Sent);
    wait_for(&rx, "reconnect after restart", connected);

    // 6. A message from "curl" arrives on the stream within a second.
    let started = Instant::now();
    post_external(addr, "hello from curl");
    wait_for(
        &rx,
        "external message",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.body == "hello from curl"),
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );

    let bodies: Vec<_> = client
        .cached_messages("general")
        .into_iter()
        .map(|m| (m.body, m.status))
        .collect();
    assert_eq!(
        bodies,
        [
            ("first".to_string(), MessageStatus::Sent),
            ("second".to_string(), MessageStatus::Sent),
            ("hello from curl".to_string(), MessageStatus::Sent),
        ]
    );

    // Leaving the room ends the Rust pump within a heartbeat or two.
    client.unsubscribe("general").unwrap();
    let deadline = Instant::now() + WAIT;
    while client.live_pumps() > 0 {
        assert!(Instant::now() < deadline, "pump did not stop");
        std::thread::sleep(Duration::from_millis(10));
    }
    client.shutdown();
}

/// With heartbeats far apart, an idle stream sits in a blocked socket read.
/// Leaving a room must still end its pump at once (the subscription's
/// cancel shuts that socket down), leave the other room streaming, and
/// shutdown must end the rest.
#[test]
fn leaving_a_silent_room_closes_its_connection_promptly() {
    no_proxy();
    let state = AppState::new(Duration::from_secs(60)).allow_anonymous(true);
    let (_server, addr) = Server::start(state, "127.0.0.1:0".parse().unwrap());
    let (client, rx) = client(addr);
    client.list_conversations().unwrap();

    let rooms = ["general", "random"];
    for room in rooms {
        client.subscribe(room).unwrap();
    }
    let mut connected_rooms = Vec::new();
    while connected_rooms.len() < rooms.len() {
        if let ChatEvent::StreamConnected { conversation_id } =
            rx.recv_timeout(WAIT).expect("both rooms connect")
        {
            connected_rooms.push(conversation_id);
        }
    }
    // Let both pumps finish catch-up and block reading their stream.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(client.live_pumps(), 2);

    let wait_pumps = |n: usize| {
        let started = Instant::now();
        while client.live_pumps() != n {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{} pumps still running, wanted {n}",
                client.live_pumps()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    };

    client.unsubscribe("general").unwrap();
    wait_pumps(1);

    // The other room is unaffected.
    post_external_to(addr, "random", "still streaming");
    wait_for(
        &rx,
        "message in the room still open",
        |e| matches!(e, ChatEvent::MessageReceived(m) if m.body == "still streaming"),
    );

    client.shutdown();
    wait_pumps(0);
}

/// What a phone does: create an account, then every call carries its token
/// and the server, not the client, decides who the author is.
#[test]
fn accounts_sign_every_call_and_own_the_author() {
    no_proxy();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("server.json");
    let state = AppState::open(&data, Duration::from_millis(100)).unwrap();
    let (server, addr) = Server::start(state, "127.0.0.1:0".parse().unwrap());
    let base = format!("http://{addr}");

    // Without a token nothing but account creation works.
    let anonymous = HttpRepo::with_ureq(&base);
    assert!(matches!(
        chat_core::ChatRepo::list_conversations(&anonymous),
        Err(ChatError::Unauthorized { .. })
    ));
    let ana = anonymous.register("Ana").unwrap();
    let ben = anonymous.register("Ben").unwrap();
    assert_ne!(ana.token, ben.token);
    assert_eq!(
        HttpRepo::with_ureq(&base)
            .with_token(&ana.token)
            .identity()
            .unwrap()
            .user_id,
        ana.user_id
    );
    assert!(matches!(
        HttpRepo::with_ureq(&base).with_token("forged").identity(),
        Err(ChatError::Unauthorized { .. })
    ));

    // Ben watches; Ana sends claiming to be someone else.
    let (tx, rx) = mpsc::channel();
    let ben_client = ChatClient::open(
        Arc::new(HttpRepo::with_ureq(&base).with_token(&ben.token)),
        ChatStore::in_memory(),
        ChatConfig {
            user: ben.display_name.clone(),
            user_id: ben.user_id.clone(),
            reconnect_initial: Duration::from_millis(20),
            reconnect_max: Duration::from_millis(100),
        },
        move |e| {
            let _ = tx.send(e);
        },
    )
    .unwrap();
    ben_client.list_conversations().unwrap();
    ben_client.watch("general").unwrap();
    wait_for(&rx, "Ben connected", connected);
    let stored = chat_core::ChatRepo::post_message(
        &HttpRepo::with_ureq(&base).with_token(&ana.token),
        &chat_core::OutgoingMessage {
            id: "ana-1".into(),
            conversation_id: "general".into(),
            author: "Ben".into(),
            author_id: ben.user_id.clone(),
            body: "hi Ben".into(),
        },
    )
    .unwrap();
    assert_eq!(
        (stored.author.as_str(), stored.author_id.as_str()),
        ("Ana", ana.user_id.as_str())
    );
    wait_for(
        &rx,
        "a notification for Ana's message",
        |e| matches!(e, ChatEvent::Notify(m) if m.id == "ana-1" && m.author_id == ana.user_id),
    );
    ben_client.shutdown();

    // Everything survives a restart of the server process.
    drop(server);
    let state = AppState::open(&data, Duration::from_millis(100)).unwrap();
    assert_eq!(state.account_count(), 2);
    let (_server, addr) = Server::start(state, "127.0.0.1:0".parse().unwrap());
    let again = HttpRepo::with_ureq(format!("http://{addr}")).with_token(&ana.token);
    let messages = chat_core::ChatRepo::load_messages(&again, "general", None).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].author_id, ana.user_id);
}
