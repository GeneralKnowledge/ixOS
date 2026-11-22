//! Rooms over `relay/room/1` (N4 step 2): joining by code, accounts that
//! are keys, moving a token account, and the event stream, with the host
//! and the devices as iroh endpoints in one process.

use std::time::Duration;

use axum::http::{Request, StatusCode};
use chat_server::{room, router, AppState};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use iroh::endpoint::{presets, Connection};
use iroh::{Endpoint, RelayMode, SecretKey};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn endpoint(n: u8) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&[n; 32]))
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .unwrap()
}

async fn dial(host: &iroh::protocol::Router, n: u8) -> (Endpoint, Connection) {
    let ep = endpoint(n).await;
    let conn = ep
        .connect(host.endpoint().addr(), room::ROOM_ALPN)
        .await
        .unwrap();
    (ep, conn)
}

/// One request on a new stream, as the app's client will make it.
async fn call(
    conn: &Connection,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let (send, recv) = conn.open_bi().await.unwrap();
    let io = hyper_util::rt::TokioIo::new(tokio::io::join(recv, send));
    let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(connection);
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "room")
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let response = sender
        .send_request(request.body(Full::new(Bytes::from(body))).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, value)
}

fn host_state(dir: &std::path::Path) -> AppState {
    AppState::open(dir.join("server.json"), Duration::from_millis(200)).unwrap()
}

#[tokio::test]
async fn a_device_joins_by_code_and_its_key_is_its_account() {
    let dir = tempfile::tempdir().unwrap();
    let state = host_state(dir.path());
    let host = room::serve(state.clone(), endpoint(100).await);
    let code = state.invite().unwrap();
    assert_eq!(code.split('-').count(), 4);

    let (_p, phone) = dial(&host, 1).await;
    // No account yet: nothing but joining.
    let (status, _) = call(&phone, "GET", "/rooms", None, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Accounts aren't made the old way over relay/room/1.
    let (status, _) = call(
        &phone,
        "POST",
        "/accounts",
        Some(json!({"display_name": "Ana"})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &phone,
        "POST",
        "/join",
        Some(json!({"code": "wrong-words-here-now", "display_name": "Ana"})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The code, typed with spaces and capitals, works once.
    let typed = code.replace('-', " ").to_uppercase();
    let (status, me) = call(
        &phone,
        "POST",
        "/join",
        Some(json!({"code": typed, "display_name": "Ana"})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{me}");
    assert_eq!(me["display_name"], "Ana");
    let (status, again) = call(
        &phone,
        "POST",
        "/join",
        Some(json!({"code": "x", "display_name": "Ana"})),
        &[],
    )
    .await;
    assert_eq!(
        (status, &again["user_id"]),
        (StatusCode::OK, &me["user_id"]),
        "already in"
    );
    let (_l, laptop) = dial(&host, 2).await;
    let (status, _) = call(
        &laptop,
        "POST",
        "/join",
        Some(json!({"code": code, "display_name": "Bo"})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "used up");

    // The key is the account, on every new stream and connection.
    let (status, posted) = call(
        &phone,
        "POST",
        "/rooms/general/messages",
        Some(json!({"body": "hello over iroh"})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{posted}");
    assert_eq!(posted["author_id"], me["user_id"]);
    let (_p2, phone_again) = dial(&host, 1).await;
    let (status, who) = call(&phone_again, "GET", "/me", None, &[]).await;
    assert_eq!((status, &who["user_id"]), (StatusCode::OK, &me["user_id"]));

    // Another device can't claim to be the phone by sending its key.
    let phone_key = SecretKey::from_bytes(&[1; 32]).public().to_string();
    let (status, _) = call(
        &laptop,
        "GET",
        "/me",
        None,
        &[("x-relay-room-key", &phone_key)],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Nor over the old HTTP API.
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/me")
                .header("x-relay-room-key", &phone_key)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_old_token_account_moves_to_a_key_with_its_history() {
    let dir = tempfile::tempdir().unwrap();
    let state = host_state(dir.path());
    // The old way, over HTTP: an account with a token, and a message.
    let http = router(state.clone());
    let response = http
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/accounts")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"display_name": "Ana"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let account: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let token = account["token"].as_str().unwrap().to_string();
    let bearer = format!("Bearer {token}");
    let response = http
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/rooms/general/messages")
                .header("content-type", "application/json")
                .header("authorization", &bearer)
                .body(axum::body::Body::from(
                    json!({"body": "before the move"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(room::accounts(&dir.path().join("server.json")).unwrap()[0].on_token);

    // The new app moves it over relay/room/1, with the token, once.
    let host = room::serve(state.clone(), endpoint(100).await);
    let (_p, phone) = dial(&host, 1).await;
    let (status, _) = call(
        &phone,
        "POST",
        "/accounts/claim",
        None,
        &[("authorization", "Bearer nope")],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, moved) = call(
        &phone,
        "POST",
        "/accounts/claim",
        None,
        &[("authorization", &bearer)],
    )
    .await;
    assert_eq!(
        (status, &moved["user_id"]),
        (StatusCode::OK, &account["user_id"])
    );

    // Its history is still its own, by key.
    let (status, messages) = call(&phone, "GET", "/rooms/general/messages", None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(messages[0]["body"], "before the move");
    assert_eq!(messages[0]["author_id"], account["user_id"]);
    // The token is gone: it works nowhere now.
    let response = http
        .oneshot(
            Request::builder()
                .uri("/me")
                .header("authorization", &bearer)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let lines = room::accounts(&dir.path().join("server.json")).unwrap();
    assert!(!lines[0].on_token);
    assert_eq!(
        lines[0].key.as_deref(),
        Some(&*SecretKey::from_bytes(&[1; 32]).public().to_string())
    );
    // The export keeps the history and no token.
    let export = room::export(&dir.path().join("server.json"))
        .unwrap()
        .to_string();
    assert!(export.contains("before the move"));
    assert!(!export.contains(&token));
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_event_stream_runs_over_the_room_protocol() {
    let state = AppState::new(Duration::from_millis(200));
    let host = room::serve(state.clone(), endpoint(100).await);
    let code = state.invite().unwrap();
    let (_p, phone) = dial(&host, 1).await;
    call(
        &phone,
        "POST",
        "/join",
        Some(json!({"code": code, "display_name": "Ana"})),
        &[],
    )
    .await;

    let (send, recv) = phone.open_bi().await.unwrap();
    let io = hyper_util::rt::TokioIo::new(tokio::io::join(recv, send));
    let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await.unwrap();
    tokio::spawn(connection);
    let response = sender
        .send_request(
            Request::builder()
                .uri("/rooms/general/stream")
                .header("host", "room")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    call(
        &phone,
        "POST",
        "/rooms/general/messages",
        Some(json!({"body": "live"})),
        &[],
    )
    .await;
    let mut seen = String::new();
    while !seen.contains("live") {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("an event in time")
            .unwrap()
            .unwrap();
        if let Ok(data) = frame.into_data() {
            seen.push_str(&String::from_utf8_lossy(&data));
        }
    }
    assert!(seen.contains("event: message"));
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn guessing_codes_is_limited() {
    let state = AppState::new(Duration::from_millis(200));
    let host = room::serve(state.clone(), endpoint(100).await);
    let code = state.invite().unwrap();
    let (_p, phone) = dial(&host, 1).await;
    for _ in 0..20 {
        let (status, _) = call(
            &phone,
            "POST",
            "/join",
            Some(json!({"code": "a-b-c-d", "display_name": "X"})),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    // Even the right code waits now.
    let (status, _) = call(
        &phone,
        "POST",
        "/join",
        Some(json!({"code": code, "display_name": "X"})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_code_is_given_back_when_the_account_cannot_be_saved() {
    let dir = tempfile::tempdir().unwrap();
    let state = host_state(dir.path());
    let host = room::serve(state.clone(), endpoint(100).await);
    let code = state.invite().unwrap();
    let (_p, phone) = dial(&host, 1).await;
    // A directory where the data file goes makes the save fail.
    let data = dir.path().join("server.json");
    let _ = std::fs::remove_file(&data);
    std::fs::create_dir_all(data.join("in-the-way")).unwrap();
    let join = json!({"code": code, "display_name": "Phone"});
    let (status, _) = call(&phone, "POST", "/join", Some(join.clone()), &[]).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    std::fs::remove_dir_all(&data).unwrap();
    let (status, _) = call(&phone, "POST", "/join", Some(join), &[]).await;
    assert_eq!(status, StatusCode::CREATED);
    host.shutdown().await.unwrap();
}
