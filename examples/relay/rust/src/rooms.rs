//! Rooms on Relay's security model (N4 step 3,
//! examples/relay/docs/relay-chat-on-relay.md): the chat server's HTTP API
//! over `relay/room/1`, from this device's iroh endpoint.
//!
//! [`RoomTransport`] is a `forge_net::HttpClient`, so the chat repo
//! (`chat_core::HttpRepo`) keeps its interface and only its transport
//! changes. Each request is HTTP/1.1 on a new iroh stream of one kept
//! connection to the host; the event stream is one long response. iroh
//! encrypts and authenticates the connection, and the host knows this
//! device by its key, so no token is sent.

use std::io::Read;
use std::sync::mpsc;
use std::sync::{Arc, Weak};
use std::time::Duration;

use forge_net::{CancelHandle, Header, HttpClient, NetError, Request, Response, StreamingResponse};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use iroh::endpoint::Connection;
use iroh::{EndpointAddr, PublicKey, TransportAddr};
use serde::{Deserialize, Serialize};

use crate::RelayCore;

/// The room protocol's ALPN (the chat server's `room::ROOM_ALPN`).
pub const ROOM_ALPN: &[u8] = b"forge-relay/room/1";
/// When a request names no timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// The most a whole (non-stream) answer may hold.
const MAX_ANSWER: usize = 64 * 1024 * 1024;
/// The base URL a room repo is made with: only its path is sent.
pub const ROOM_BASE_URL: &str = "http://room";

/// Where a room host is: its key, and how to reach it. With no relay
/// named, this device's own relay is used (as for the mailbox).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct RoomHost {
    pub endpoint_id: String,
    #[serde(default)]
    pub relay_url: Option<String>,
    /// Direct addresses (`ip:port`) to try as well, e.g. on the same Wi-Fi.
    #[serde(default)]
    pub addresses: Vec<String>,
}

impl RoomHost {
    fn key(&self) -> Result<PublicKey, NetError> {
        self.endpoint_id
            .parse()
            .map_err(|_| NetError::InvalidRequest {
                reason: "not a room host's key".into(),
            })
    }
}

/// Where a room transport finds Relay's core: set once Relay is open
/// (`ChatCore::use_relay`), which may be after the chat's session opened.
pub type RelaySlot = Arc<std::sync::Mutex<Weak<RelayCore>>>;

/// The chat server's API over `relay/room/1`, from Relay's node.
pub struct RoomTransport {
    core: RelaySlot,
    host: RoomHost,
    /// The connection to the host, kept between requests.
    kept: Arc<tokio::sync::Mutex<Option<Connection>>>,
}

impl RoomTransport {
    pub fn new(core: &Arc<RelayCore>, host: RoomHost) -> Self {
        Self::with_slot(Arc::new(std::sync::Mutex::new(Arc::downgrade(core))), host)
    }

    /// A transport that uses whatever Relay core `slot` holds when it asks.
    pub fn with_slot(slot: RelaySlot, host: RoomHost) -> Self {
        Self {
            core: slot,
            host,
            kept: Arc::default(),
        }
    }

    /// Runs `work` on Relay's runtime and waits for it here (a worker
    /// thread of the chat engine, never the runtime's own).
    /// A cancel ends the wait at once with `Cancelled`.
    fn run<T: Send + 'static>(
        &self,
        timeout: Duration,
        cancel: Option<&CancelHandle>,
        work: impl std::future::Future<Output = Result<T, NetError>> + Send + 'static,
    ) -> Result<T, NetError> {
        let (tx, rx) = mpsc::channel();
        let cancelled = tx.clone();
        let _registration =
            cancel.map(|c| c.on_cancel(move || drop(cancelled.send(Err(NetError::Cancelled)))));
        let task = crate::code::TOKIO.spawn(async move {
            let result = tokio::time::timeout(timeout, work)
                .await
                .unwrap_or(Err(NetError::Timeout));
            let _ = tx.send(result);
        });
        let result = rx.recv().unwrap_or(Err(NetError::Transport {
            reason: "Relay closed".into(),
        }));
        task.abort();
        result
    }

    /// What one request needs on the runtime: the core, the host's
    /// address, and the kept connection.
    fn parts(&self) -> Result<Parts, NetError> {
        let core = forge_runtime::lock(&self.core)
            .upgrade()
            .ok_or(NetError::Offline {
                reason: "Relay isn't open".into(),
            })?;
        Ok(Parts {
            core,
            key: self.host.key()?,
            host: self.host.clone(),
            kept: Arc::clone(&self.kept),
        })
    }
}

struct Parts {
    core: Arc<RelayCore>,
    key: PublicKey,
    host: RoomHost,
    kept: Arc<tokio::sync::Mutex<Option<Connection>>>,
}

impl Parts {
    /// The kept connection, or a new one.
    async fn connection(&self) -> Result<Connection, NetError> {
        let mut kept = self.kept.lock().await;
        if let Some(conn) = kept.as_ref() {
            if conn.close_reason().is_none() {
                return Ok(conn.clone());
            }
        }
        self.core
            .device_service()
            .await
            .map_err(|e| NetError::Offline {
                reason: e.to_string(),
            })?;
        let node = self.core.node.started().ok_or(NetError::Offline {
            reason: "Relay isn't open to devices".into(),
        })?;
        let relay = match &self.host.relay_url {
            Some(url) => Some(url.parse().map_err(|_| NetError::InvalidRequest {
                reason: "not a relay URL".into(),
            })?),
            None => node.relay(),
        };
        let addresses = self
            .host
            .addresses
            .iter()
            .filter_map(|a| a.parse().ok())
            .map(TransportAddr::Ip);
        let addr = EndpointAddr::from_parts(
            self.key,
            relay.map(TransportAddr::Relay).into_iter().chain(addresses),
        );
        let conn = node
            .endpoint
            .connect(addr, ROOM_ALPN)
            .await
            .map_err(|e| NetError::Offline {
                reason: format!("the room's host wasn't reached: {e}"),
            })?;
        *kept = Some(conn.clone());
        Ok(conn)
    }

    /// Sends `request` on a new stream and returns the answer's head, with
    /// its body still to read.
    async fn exchange(
        &self,
        request: http::Request<Full<Bytes>>,
    ) -> Result<http::Response<hyper::body::Incoming>, NetError> {
        let conn = self.connection().await?;
        let (send, recv) = conn.open_bi().await.map_err(transport)?;
        let io = hyper_util::rt::TokioIo::new(tokio::io::join(recv, send));
        let (mut sender, connection) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(transport)?;
        tokio::spawn(connection);
        sender.send_request(request).await.map_err(transport)
    }
}

fn transport(e: impl std::fmt::Display) -> NetError {
    NetError::Transport {
        reason: e.to_string(),
    }
}

/// `request` as hyper sends it: the URL's path and query only.
fn to_hyper(request: &Request) -> Result<http::Request<Full<Bytes>>, NetError> {
    let url = url::Url::parse(&request.url).map_err(|e| NetError::InvalidRequest {
        reason: e.to_string(),
    })?;
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_string(),
    };
    let mut builder = http::Request::builder()
        .method(request.method.as_str())
        .uri(path)
        .header("host", "room");
    for Header { name, value } in &request.headers {
        // The connection is the account: a token means nothing here, and
        // isn't sent (except to move an old account, `/accounts/claim`).
        if name.eq_ignore_ascii_case("authorization") && !url.path().ends_with("/accounts/claim") {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
        .body(Full::new(Bytes::from(request.body.clone())))
        .map_err(|e| NetError::InvalidRequest {
            reason: e.to_string(),
        })
}

fn headers(head: &http::HeaderMap) -> Vec<Header> {
    head.iter()
        .filter_map(|(name, value)| {
            Some(Header {
                name: name.as_str().to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect()
}

impl RoomTransport {
    /// One request and its whole answer.
    fn exchange(
        &self,
        request: &Request,
        cancel: Option<&CancelHandle>,
    ) -> Result<Response, NetError> {
        let parts = self.parts()?;
        let hyper_request = to_hyper(request)?;
        self.run(
            request.timeout.unwrap_or(DEFAULT_TIMEOUT),
            cancel,
            async move {
                let response = parts.exchange(hyper_request).await?;
                let status = response.status().as_u16();
                let headers = headers(response.headers());
                let body = http_body_util::Limited::new(response.into_body(), MAX_ANSWER)
                    .collect()
                    .await
                    .map_err(transport)?
                    .to_bytes()
                    .to_vec();
                Ok(Response {
                    status,
                    headers,
                    body,
                })
            },
        )
    }
}

impl HttpClient for RoomTransport {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        self.exchange(request, None)
    }

    fn send_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<Response, NetError> {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        self.exchange(request, Some(cancel))
    }

    fn open_stream(&self, request: &Request) -> Result<StreamingResponse, NetError> {
        self.open_stream_cancellable(request, &CancelHandle::new())
    }

    fn open_stream_cancellable(
        &self,
        request: &Request,
        cancel: &CancelHandle,
    ) -> Result<StreamingResponse, NetError> {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let parts = self.parts()?;
        let hyper_request = to_hyper(request)?;
        let response = self.run(
            request.timeout.unwrap_or(DEFAULT_TIMEOUT),
            Some(cancel),
            async move { parts.exchange(hyper_request).await },
        )?;
        let status = response.status().as_u16();
        let headers = headers(response.headers());
        // The body comes through a small channel, read on the runtime; a
        // cancel stops that task, and the reader ends.
        let (tx, rx) = mpsc::sync_channel::<Result<Bytes, String>>(16);
        let mut body = response.into_body();
        let pump = crate::code::TOKIO.spawn(async move {
            while let Some(frame) = body.frame().await {
                let item = match frame {
                    Ok(frame) => match frame.into_data() {
                        Ok(data) => Ok(data),
                        Err(_) => continue,
                    },
                    Err(e) => Err(e.to_string()),
                };
                let failed = item.is_err();
                let tx = tx.clone();
                // A full channel waits off the runtime's threads.
                let sent = tokio::task::spawn_blocking(move || tx.send(item).is_ok())
                    .await
                    .unwrap_or(false);
                if !sent || failed {
                    return;
                }
            }
        });
        let pump = pump.abort_handle();
        let abort = pump.clone();
        let registration = cancel.on_cancel(move || abort.abort());
        Ok(StreamingResponse {
            status,
            headers,
            body: Box::new(StreamBody {
                rx,
                pending: Bytes::new(),
                cancel: cancel.clone(),
                pump,
                _registration: registration,
            }),
        })
    }
}

/// A response body read as it arrives.
struct StreamBody {
    rx: mpsc::Receiver<Result<Bytes, String>>,
    pending: Bytes,
    cancel: CancelHandle,
    /// Stopped when the body is dropped, even mid-wait for a frame.
    pump: tokio::task::AbortHandle,
    _registration: forge_net::AbortRegistration,
}

impl Drop for StreamBody {
    fn drop(&mut self) {
        self.pump.abort();
    }
}

impl Read for StreamBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.pending.is_empty() {
            if self.cancel.is_cancelled() {
                return Err(forge_net::cancelled_io_error());
            }
            match self.rx.recv() {
                Ok(Ok(bytes)) => self.pending = bytes,
                Ok(Err(e)) => return Err(std::io::Error::other(e)),
                Err(_) if self.cancel.is_cancelled() => return Err(forge_net::cancelled_io_error()),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending = self.pending.slice(n..);
        Ok(n)
    }
}

/// Kept so a test or the CLI can make a repo the way the app does.
pub fn room_repo(core: &Arc<RelayCore>, host: RoomHost) -> chat_core::HttpRepo {
    chat_core::HttpRepo::new(Arc::new(RoomTransport::new(core, host)), ROOM_BASE_URL)
}

/// This device's account in a room host's rooms: its key, and the name
/// the others see.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct RoomAccount {
    pub user_id: String,
    pub display_name: String,
}

/// Joins `host`'s rooms with a word code from `chat-server invite`: the
/// host makes an account for this device's key. Joining again (already
/// in) answers with the same account, whatever the code.
pub fn join(
    core: &Arc<RelayCore>,
    host: RoomHost,
    code: &str,
    display_name: &str,
) -> Result<RoomAccount, chat_core::ChatError> {
    join_with(&RoomTransport::new(core, host), code, display_name)
}

pub(crate) fn join_with(
    transport: &RoomTransport,
    code: &str,
    display_name: &str,
) -> Result<RoomAccount, chat_core::ChatError> {
    let display_name = chat_core::validate_display_name(display_name)?;
    let body = serde_json::json!({ "code": code, "display_name": display_name });
    let request = Request::post(
        format!("{ROOM_BASE_URL}/join"),
        body.to_string().into_bytes(),
    )
    .header("Content-Type", "application/json")
    .header("Accept", "application/json");
    answer(transport.send(&request)?)
}

/// Moves an old token account to this device's key, with its rooms and
/// history: the token goes once, encrypted, and the host deletes it. An
/// empty token asks which account this key already has.
pub(crate) fn claim_with(
    transport: &RoomTransport,
    token: &str,
) -> Result<RoomAccount, chat_core::ChatError> {
    let mut request = Request::post(format!("{ROOM_BASE_URL}/accounts/claim"), Vec::new())
        .header("Accept", "application/json");
    // No token: the host answers with the account already on this key.
    if !token.is_empty() {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    answer(transport.send(&request)?)
}

fn answer(response: Response) -> Result<RoomAccount, chat_core::ChatError> {
    match response.status {
        200..=299 => {
            serde_json::from_slice(&response.body).map_err(|e| chat_core::ChatError::Protocol {
                reason: e.to_string(),
            })
        }
        status => Err(chat_core::ChatError::Http {
            status,
            // cut: a server's error body, shown only as an error's reason
            reason: String::from_utf8_lossy(&response.body)
                .trim()
                .chars()
                .take(200)
                .collect(),
        }),
    }
}
