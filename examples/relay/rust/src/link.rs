//! One Relay connection, end to end, over blocking TCP: dialling and
//! sending, or answering and receiving. No runtime and no saved state:
//! the core passes in what it knows and hears back through [`Inbound`].
//! The `relay` command and the tests use the same code.

use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use forge_devproto::conn::Conn;
use forge_devproto::secure::{Purpose as Keyed, Role};
use forge_storage::FileStore;

use crate::model::{RelayError, MAX_FILE_BYTES};
use crate::wire::{Intro, Msg, Offered, Purpose, CHUNK, MAGIC, PROTOCOL};

/// How long to wait to connect, for each handshake step and each reply.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The stream's read timeout: how often a waiting loop can check for
/// cancellation.
const POLL: Duration = Duration::from_millis(200);

/// Who this device is.
#[derive(Clone, Debug)]
pub struct Me {
    pub device_id: String,
    pub name: String,
}

/// The other end, as it introduced itself after the handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Them {
    pub device_id: String,
    pub name: String,
}

/// An open, encrypted connection from this device.
pub struct Link {
    conn: Conn<TcpStream>,
    pub them: Them,
    /// The address that answered.
    pub address: String,
}

/// Something to send.
pub enum Outgoing<'a> {
    Note(&'a str),
    File {
        path: &'a Path,
        name: &'a str,
        size: u64,
        sha256: &'a str,
    },
}

fn connect(addresses: &[String]) -> Result<(TcpStream, String), RelayError> {
    let mut last = String::from("no address");
    for address in addresses {
        let resolved = match address.to_socket_addrs() {
            Ok(mut it) => it.next(),
            Err(err) => {
                last = err.to_string();
                continue;
            }
        };
        let Some(socket) = resolved else { continue };
        match TcpStream::connect_timeout(&socket, TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(POLL))?;
                stream.set_nodelay(true)?;
                return Ok((stream, address.clone()));
            }
            Err(err) => last = err.to_string(),
        }
    }
    Err(RelayError::Unreachable { reason: last })
}

impl Link {
    /// Connects to the first of `addresses` that answers, proves this side
    /// knows `secret`, and swaps names. `expect` is the device it must be.
    pub fn dial(
        addresses: &[String],
        me: &Me,
        purpose: Purpose,
        secret: &[u8; 32],
        expect: &str,
    ) -> Result<Self, RelayError> {
        let (stream, address) = connect(addresses)?;
        let mut conn = Conn::new(stream);
        conn.send_clear(&Intro {
            magic: MAGIC.into(),
            protocol: PROTOCOL,
            from: me.device_id.clone(),
            purpose,
        })?;
        conn.handshake(Role::Host, Keyed::Session(secret), TIMEOUT)?;
        conn.send(&Msg::Hello {
            device_id: me.device_id.clone(),
            name: me.name.clone(),
        })?;
        let them = match conn.recv_within::<Msg>(TIMEOUT)? {
            Msg::Hello { device_id, name } => Them { device_id, name },
            _ => return Err(protocol("expected hello")),
        };
        if them.device_id != expect {
            return Err(RelayError::Refused);
        }
        Ok(Self {
            conn,
            them,
            address,
        })
    }

    /// Sends one item and waits for the other side to keep it. `progress`
    /// hears bytes sent so far; `cancelled` is asked between chunks.
    pub fn send(
        &mut self,
        item: Outgoing<'_>,
        mut progress: impl FnMut(u64),
        cancelled: impl Fn() -> bool,
    ) -> Result<(), RelayError> {
        let offer = match &item {
            Outgoing::Note(text) => Offered::Note {
                text: (*text).into(),
            },
            Outgoing::File {
                name, size, sha256, ..
            } => Offered::File {
                name: (*name).into(),
                size: *size,
                sha256: (*sha256).into(),
            },
        };
        self.conn.send(&Msg::Offer { item: offer })?;
        match self.wait(&cancelled)? {
            Msg::Accept => {}
            Msg::Decline { reason } => return Err(RelayError::Declined { reason }),
            _ => return Err(protocol("expected an answer to the offer")),
        }
        if let Outgoing::File { path, size, .. } = item {
            let mut file = File::open(path).map_err(|err| RelayError::File {
                reason: err.to_string(),
            })?;
            let mut buf = vec![0u8; CHUNK];
            let mut sent = 0u64;
            loop {
                if cancelled() {
                    return Err(stopped());
                }
                let n = file.read(&mut buf).map_err(|err| RelayError::File {
                    reason: err.to_string(),
                })?;
                if n == 0 {
                    break;
                }
                self.conn.send(&Msg::Chunk {
                    data: buf[..n].to_vec(),
                })?;
                sent += n as u64;
                progress(sent);
            }
            if sent != size {
                return Err(RelayError::File {
                    reason: "the file changed while it was being sent".into(),
                });
            }
            self.conn.send(&Msg::Done)?;
        }
        match self.wait(&cancelled)? {
            Msg::Received { ok: true, .. } => Ok(()),
            Msg::Received { reason, .. } => Err(RelayError::Declined {
                reason: reason.unwrap_or_else(|| "it didn't arrive intact".into()),
            }),
            _ => Err(protocol("expected a receipt")),
        }
    }

    /// Says goodbye; the other side closes.
    pub fn close(mut self) {
        let _ = self.conn.send(&Msg::Bye);
    }

    fn wait(&mut self, cancelled: &impl Fn() -> bool) -> Result<Msg, RelayError> {
        let deadline = std::time::Instant::now() + TIMEOUT * 6;
        loop {
            if cancelled() {
                return Err(stopped());
            }
            if let Some(msg) = self.conn.recv::<Msg>()? {
                return Ok(msg);
            }
            if std::time::Instant::now() >= deadline {
                return Err(RelayError::Connection {
                    reason: "the other device stopped answering".into(),
                });
            }
        }
    }
}

/// What a listening device decides and hears about, for one connection.
pub trait Inbound {
    /// The secret for a dialler's intro: the open invitation's for
    /// pairing, the stored one for a paired device. `None` hangs up.
    fn secret(&self, intro: &Intro) -> Option<[u8; 32]>;
    /// The handshake passed. For pairing, the invitation is now used.
    fn introduced(&self, purpose: &Purpose, secret: &[u8; 32], them: &Them, address: &str);
    /// An item is on its way; returns a history id, or a reason to decline.
    fn offered(&self, them: &Them, item: &Offered) -> Result<u64, String>;
    fn progress(&self, id: u64, done: u64);
    /// It arrived (`Ok` with the received file's inbox name, if any), or
    /// didn't.
    fn finished(&self, id: u64, result: Result<Option<String>, String>);
}

/// Answers one connection: checks the intro, runs the handshake, swaps
/// names, then takes items until the dialler says goodbye or hangs up.
pub fn answer(
    stream: TcpStream,
    me: &Me,
    inbox: &FileStore,
    host: &impl Inbound,
) -> Result<(), RelayError> {
    stream.set_read_timeout(Some(POLL))?;
    stream.set_nodelay(true)?;
    let address = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let mut conn = Conn::new(stream);
    let intro: Intro = forge_devproto::decode(&conn.recv_clear_bytes(TIMEOUT)?)
        .map_err(|_| protocol("not a Relay device"))?;
    if intro.magic != MAGIC || intro.protocol != PROTOCOL {
        return Err(protocol("not a Relay device, or a different version"));
    }
    let secret = host.secret(&intro).ok_or(RelayError::Refused)?;
    conn.handshake(Role::App, Keyed::Session(&secret), TIMEOUT)?;
    let them = match conn.recv_within::<Msg>(TIMEOUT)? {
        Msg::Hello { device_id, name } if device_id == intro.from => Them { device_id, name },
        _ => return Err(protocol("expected hello")),
    };
    conn.send(&Msg::Hello {
        device_id: me.device_id.clone(),
        name: me.name.clone(),
    })?;
    host.introduced(&intro.purpose, &secret, &them, &address);

    loop {
        let msg = match conn.recv::<Msg>() {
            Ok(Some(msg)) => msg,
            Ok(None) => continue,
            Err(forge_devproto::conn::ConnError::Closed) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        match msg {
            Msg::Bye => return Ok(()),
            Msg::Offer { item } => {
                let id = match host.offered(&them, &item) {
                    Ok(id) => id,
                    Err(reason) => {
                        conn.send(&Msg::Decline { reason })?;
                        continue;
                    }
                };
                conn.send(&Msg::Accept)?;
                let result = match &item {
                    Offered::Note { .. } => Ok(None),
                    Offered::File { name, size, sha256 } => {
                        receive_file(&mut conn, inbox, name, *size, sha256, |done| {
                            host.progress(id, done)
                        })
                        .map(Some)
                    }
                };
                let reply = Msg::Received {
                    ok: result.is_ok(),
                    reason: result.as_ref().err().map(|e| e.to_string()),
                };
                host.finished(id, result.map_err(|e| e.to_string()));
                conn.send(&reply)?;
            }
            _ => return Err(protocol("unexpected message")),
        }
    }
}

/// Takes a file's chunks into the inbox. It is kept, as
/// `<sha256>/<name>` so the name and extension survive, only if its size
/// and SHA-256 are what the sender announced.
fn receive_file(
    conn: &mut Conn<TcpStream>,
    inbox: &FileStore,
    name: &str,
    size: u64,
    sha256: &str,
    mut progress: impl FnMut(u64),
) -> Result<String, RelayError> {
    if size > MAX_FILE_BYTES {
        return Err(RelayError::FileTooLarge);
    }
    let partial = inbox.partial_path()?;
    let result = (|| {
        let mut out = File::create(&partial).map_err(|err| RelayError::File {
            reason: err.to_string(),
        })?;
        let mut got = 0u64;
        let deadline_gap = TIMEOUT * 6;
        let mut last = std::time::Instant::now();
        loop {
            let msg = match conn.recv::<Msg>()? {
                Some(msg) => msg,
                None if last.elapsed() > deadline_gap => {
                    return Err(RelayError::Connection {
                        reason: "the sender stopped".into(),
                    })
                }
                None => continue,
            };
            last = std::time::Instant::now();
            match msg {
                Msg::Chunk { data } => {
                    got += data.len() as u64;
                    if got > size {
                        return Err(protocol("more bytes than announced"));
                    }
                    out.write_all(&data).map_err(|err| RelayError::File {
                        reason: err.to_string(),
                    })?;
                    progress(got);
                }
                Msg::Done => break,
                _ => return Err(protocol("expected file data")),
            }
        }
        out.sync_all().map_err(|err| RelayError::File {
            reason: err.to_string(),
        })?;
        if got != size {
            return Err(protocol("fewer bytes than announced"));
        }
        let actual = forge_storage::files::sha256_file(&partial)?;
        if actual != sha256 {
            return Err(RelayError::File {
                reason: "it changed on the way".into(),
            });
        }
        let kept = format!("{sha256}/{}", forge_storage::files::safe_file_name(name));
        Ok(inbox.accept(&partial, &kept)?.name)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn protocol(what: &str) -> RelayError {
    RelayError::Connection {
        reason: what.into(),
    }
}

fn stopped() -> RelayError {
    RelayError::Stopped {
        kind: forge_app::ErrorKind::Cancelled,
    }
}
