//! Rooms on Relay's security model (N4 step 2,
//! examples/relay/docs/relay-chat-on-relay.md): the chat server over
//! `relay/room/1`, an iroh protocol, with accounts that are keys and
//! joining by code.
//!
//! - **Transport.** Each iroh stream carries the same HTTP/1.1 API as
//!   before (requests, and the event stream as `text/event-stream`), so
//!   every feature stays as it is. iroh encrypts and authenticates the
//!   connection, so nobody between a device and the host reads or changes
//!   it, and there is no TLS to set up.
//! - **Your account is your key.** The connection proves the caller's
//!   endpoint id; the transport puts it on each request in
//!   `x-relay-room-key`, and [`caller_header`] removes that header from
//!   anything that didn't come that way. A token counts for nothing on
//!   `relay/room/1`, except to move an old account ([`claim`]).
//! - **Joining is by code.** `chat-server invite` makes a one-time word
//!   code (four words, ten minutes). `POST /join` with it makes an account
//!   for the caller's key. Only the code's SHA-256 is kept, and wrong codes
//!   are limited server-wide, so guessing one in its ten minutes isn't
//!   practical.
//! - **Moving an old account.** Over `relay/room/1`, `POST /accounts/claim`
//!   with the old `Authorization: Bearer` token ties that account to the
//!   caller's key and deletes the token. Its rooms, messages and read
//!   state stay. The token never crosses the network in the clear again.

use std::path::{Path, PathBuf};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, SecretKey};
use serde::{Deserialize, Serialize};

use super::{
    clean_display_name, load, now_ms, random_hex, storage_failed, write_private_atomically,
    AppState, DataError, User, WireIdentity,
};

/// The room protocol's ALPN.
pub const ROOM_ALPN: &[u8] = b"forge-relay/room/1";
/// Set by the room transport to the caller's endpoint id; never trusted
/// from anywhere else.
const KEY_HEADER: &str = "x-relay-room-key";
/// How long an invitation code lasts.
pub const INVITE_LIFETIME_MS: i64 = 10 * 60 * 1000;
/// Wrong codes the server takes in a minute, from everyone together.
const WRONG_CODES_PER_MINUTE: usize = 20;
/// Words in an invitation code.
const CODE_WORDS: usize = 4;

static WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    include_str!("../../../rust/src/eff_short_wordlist_1.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty() && !l.contains('-'))
        .collect()
});

/// The caller's endpoint id, put on a request by the room transport.
#[derive(Clone, Debug)]
pub struct RoomPeer(pub String);

/// The caller's key, if the request came over `relay/room/1`.
pub(crate) fn room_key(headers: &HeaderMap) -> Option<&str> {
    headers.get(KEY_HEADER).and_then(|v| v.to_str().ok())
}

/// Sets `x-relay-room-key` from the connection on a room request, and
/// removes it from every other request, so only the transport can say it.
pub(crate) async fn caller_header(mut request: Request) -> Request {
    request.headers_mut().remove(KEY_HEADER);
    let peer = request.extensions().get::<RoomPeer>().cloned();
    if let Some(RoomPeer(key)) = peer {
        if let Ok(value) = HeaderValue::from_str(&key) {
            request.headers_mut().insert(KEY_HEADER, value);
        }
    }
    request
}

/// An invitation, as kept: the code's SHA-256 only.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Invite {
    code_sha256: String,
    expires_at: i64,
}

fn code_digest(code: &str) -> String {
    use sha2::{Digest, Sha256};
    let normal = code
        .split(|c: char| c == '-' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-");
    Sha256::digest(normal.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn new_code() -> std::io::Result<String> {
    let mut words = Vec::with_capacity(CODE_WORDS);
    for _ in 0..CODE_WORDS {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(std::io::Error::other)?;
        words.push(WORDS[u32::from_le_bytes(b) as usize % WORDS.len()]);
    }
    Ok(words.join("-"))
}

/// The file invitations are kept in, next to the data file, so the
/// `chat-server invite` command can add one while the server runs.
pub fn invites_file(data_file: &Path) -> PathBuf {
    data_file.with_file_name("invites.json")
}

fn read_invites(path: &Path) -> Vec<Invite> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_invites(path: &Path, invites: &[Invite]) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(invites).map_err(std::io::Error::other)?;
    write_private_atomically(path, &json)
}

/// Changes the invitations file under a lock shared with every process
/// (the server and `chat-server invite`), held across the whole read,
/// change and write, so neither undoes the other's change.
fn with_invites<T>(
    data_file: &Path,
    change: impl FnOnce(&mut Vec<Invite>) -> T,
) -> std::io::Result<T> {
    let path = invites_file(data_file);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    let mut invites = read_invites(&path);
    let before = invites.clone();
    let value = change(&mut invites);
    if invites != before {
        write_invites(&path, &invites)?;
    }
    Ok(value)
}

/// Adds a one-time invitation for the server whose data is `data_file`,
/// and returns its code. Expired ones are dropped on the way.
pub fn add_invite(data_file: &Path) -> std::io::Result<String> {
    let code = new_code()?;
    let now = now_ms();
    with_invites(data_file, |invites| {
        invites.retain(|i| i.expires_at > now);
        invites.push(Invite {
            code_sha256: code_digest(&code),
            expires_at: now + INVITE_LIFETIME_MS,
        });
    })?;
    Ok(code)
}

/// Removes the live invitation `digest` names, and returns it.
fn take(invites: &mut Vec<Invite>, digest: &str, now: i64) -> Option<Invite> {
    invites.retain(|i| i.expires_at > now);
    let at = invites
        .iter()
        .position(|i| super::constant_time_eq(&i.code_sha256, digest))?;
    Some(invites.remove(at))
}

impl super::Inner {
    /// Uses up `code` if it is a live invitation, and returns it so it can
    /// be put back if the account can't be saved.
    fn take_invite(&mut self, code: &str) -> std::io::Result<Option<Invite>> {
        let digest = code_digest(code);
        let now = now_ms();
        match self.data_file.clone() {
            Some(data_file) => with_invites(&data_file, |invites| take(invites, &digest, now)),
            None => Ok(take(&mut self.invites, &digest, now)),
        }
    }

    /// Puts back an invitation taken for an account that couldn't be saved.
    fn restore_invite(&mut self, invite: Invite) {
        match self.data_file.clone() {
            Some(data_file) => {
                if let Err(err) = with_invites(&data_file, |invites| invites.push(invite)) {
                    eprintln!("error: couldn't put an invitation back: {err}");
                }
            }
            None => self.invites.push(invite),
        }
    }
}

impl AppState {
    /// A one-time invitation code for this server (kept in memory for an
    /// in-memory server, else in the invitations file).
    pub fn invite(&self) -> std::io::Result<String> {
        let data_file = self.lock().data_file.clone();
        match data_file {
            Some(path) => add_invite(&path),
            None => {
                let code = new_code()?;
                self.lock().invites.push(Invite {
                    code_sha256: code_digest(&code),
                    expires_at: now_ms() + INVITE_LIFETIME_MS,
                });
                Ok(code)
            }
        }
    }

    /// Counts a wrong code; false when too many came in the last minute.
    fn wrong_code_allowed(&self) -> bool {
        let now = now_ms();
        let mut inner = self.lock();
        inner.wrong_codes.retain(|at| *at > now - 60_000);
        inner.wrong_codes.len() < WRONG_CODES_PER_MINUTE
    }
}

#[derive(Deserialize)]
pub(crate) struct JoinBody {
    code: String,
    display_name: String,
}

fn identity(user: &User) -> Json<WireIdentity> {
    Json(WireIdentity {
        user_id: user.id.clone(),
        display_name: user.display_name.clone(),
    })
}

/// `POST /join` over `relay/room/1`: `{"code","display_name"}` → `201`
/// `{"user_id","display_name"}`, an account for the caller's key. A key
/// that already has one gets it back (`200`), and the code isn't used up.
pub(crate) async fn join(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<JoinBody>,
) -> Response {
    let Some(key) = room_key(&headers).map(str::to_string) else {
        return (StatusCode::FORBIDDEN, "join over Relay, not HTTP").into_response();
    };
    if let Some(user) = state.lock().user_by_key(&key) {
        return identity(user).into_response();
    }
    let display_name = match clean_display_name(&body.display_name) {
        Ok(name) => name,
        Err(why) => return (StatusCode::BAD_REQUEST, why).into_response(),
    };
    if !state.wrong_code_allowed() {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many wrong codes; try again in a minute",
        )
            .into_response();
    }
    // Everything that can fail without using the code comes first.
    let id = match random_hex(8) {
        Ok(id) => format!("u-{id}"),
        Err(err) => return storage_failed(err),
    };
    // One lock from the check for an account to the new one being saved,
    // so two joins by one key can't both use a code.
    let mut inner = state.lock();
    if let Some(user) = inner.user_by_key(&key) {
        return identity(user).into_response();
    }
    let invite = match inner.take_invite(&body.code) {
        Ok(Some(invite)) => invite,
        Ok(None) => {
            inner.wrong_codes.push(now_ms());
            return (
                StatusCode::FORBIDDEN,
                "that code isn't right, or it has expired",
            )
                .into_response();
        }
        Err(err) => return storage_failed(err),
    };
    let user = User {
        id,
        display_name,
        token: String::new(),
        created_at: now_ms(),
        key: Some(key),
    };
    inner.users.push(user.clone());
    if let Err(err) = inner.save() {
        inner.users.pop();
        inner.restore_invite(invite);
        return storage_failed(err);
    }
    (StatusCode::CREATED, identity(&user)).into_response()
}

/// `POST /accounts/claim` over `relay/room/1`, with the account's old
/// `Authorization: Bearer` token: the account becomes the caller's key, and
/// the token is deleted. `200` `{"user_id","display_name"}`.
pub(crate) async fn claim(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(key) = room_key(&headers).map(str::to_string) else {
        return (
            StatusCode::FORBIDDEN,
            "move an account over Relay, not HTTP",
        )
            .into_response();
    };
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let mut inner = state.lock();
    let Some(token) = token else {
        // Already moved: the key is the account.
        return match inner.user_by_key(&key) {
            Some(user) => identity(user).into_response(),
            None => (StatusCode::UNAUTHORIZED, "no account token to move").into_response(),
        };
    };
    let Some(id) = inner.user_by_token(token).map(|u| u.id.clone()) else {
        return (StatusCode::UNAUTHORIZED, "unknown token").into_response();
    };
    if inner.user_by_key(&key).is_some_and(|u| u.id != id) {
        return (
            StatusCode::CONFLICT,
            "this device already has another account here",
        )
            .into_response();
    }
    let before = inner.users.clone();
    let user = inner
        .users
        .iter_mut()
        .find(|u| u.id == id)
        .expect("found above");
    user.key = Some(key);
    user.token.clear();
    let user = user.clone();
    if let Err(err) = inner.save() {
        inner.users = before;
        return storage_failed(err);
    }
    identity(&user).into_response()
}

/// One account, as `chat-server accounts` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLine {
    pub user_id: String,
    pub display_name: String,
    /// The endpoint id it is on `relay/room/1`, once moved or joined.
    pub key: Option<String>,
    /// Still has an old bearer token.
    pub on_token: bool,
}

/// The accounts in a data file, without starting a server.
pub fn accounts(data_file: &Path) -> Result<Vec<AccountLine>, DataError> {
    Ok(load(data_file)?
        .users
        .into_iter()
        .map(|u| AccountLine {
            on_token: !u.token.is_empty(),
            user_id: u.id,
            display_name: u.display_name,
            key: u.key,
        })
        .collect())
}

/// Every room's full history in a data file, as JSON, without the
/// accounts' tokens: for keeping a server you retire as a file.
pub fn export(data_file: &Path) -> Result<serde_json::Value, DataError> {
    let stored = load(data_file)?;
    let accounts: Vec<_> = stored
        .users
        .iter()
        .map(|u| serde_json::json!({"user_id": u.id, "display_name": u.display_name, "key": u.key}))
        .collect();
    let rooms: Vec<_> = stored
        .rooms
        .iter()
        .map(|r| serde_json::json!({"id": r.id, "title": r.title, "messages": r.messages}))
        .collect();
    Ok(serde_json::json!({
        "exported_at": now_ms(),
        "accounts": accounts,
        "rooms": rooms,
    }))
}

/// The host's key: `room.key` in `dir`, made (owner-only) the first time.
pub fn host_key(dir: &Path) -> std::io::Result<SecretKey> {
    let path = dir.join("room.key");
    match std::fs::read(&path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| std::io::Error::other("room.key isn't a key"))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
            write_private_atomically(&path, &bytes)?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) => Err(e),
    }
}

/// `relay/room/1`: each stream an HTTP/1.1 connection to the chat API,
/// with the caller's key on every request.
#[derive(Clone)]
pub struct RoomHandler {
    app: Router,
}

impl std::fmt::Debug for RoomHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RoomHandler")
    }
}

impl RoomHandler {
    /// The handler for `state`'s rooms.
    pub fn new(state: AppState) -> Self {
        Self {
            app: super::router(state),
        }
    }
}

impl ProtocolHandler for RoomHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let peer = conn.remote_id().to_string();
        while let Ok((send, recv)) = conn.accept_bi().await {
            let app = self.app.clone();
            let peer = peer.clone();
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(tokio::io::join(recv, send));
                let service = hyper::service::service_fn(
                    move |request: hyper::Request<hyper::body::Incoming>| {
                        let mut request = request.map(axum::body::Body::new);
                        request.extensions_mut().insert(RoomPeer(peer.clone()));
                        tower::ServiceExt::oneshot(app.clone(), request)
                    },
                );
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                {
                    eprintln!("room: a stream ended: {err}");
                }
            });
        }
        Ok(())
    }
}

/// Serves `state`'s rooms on `relay/room/1` from `endpoint`, until the
/// returned router is dropped or shut down.
pub fn serve(state: AppState, endpoint: Endpoint) -> iroh::protocol::Router {
    iroh::protocol::Router::builder(endpoint)
        .accept(ROOM_ALPN, RoomHandler::new(state))
        .spawn()
}
