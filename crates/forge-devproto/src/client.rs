//! The host end of a connection, over any blocking stream: pairing, opening
//! a session, commands and the live record stream. `forge device` runs it
//! over usbmuxd; the app side's tests run it over loopback TCP.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::conn::{Conn, ConnError};
use crate::secure::{Purpose, Role};
use crate::{
    AppMsg, ByeReason, Command, Hello, HostMsg, Open, Preamble, Record, Reply, Secret, SECRET_LEN,
};

/// How long each step of opening a connection may take.
const STEP: Duration = Duration::from_secs(5);
/// How long a command may take to answer.
const COMMAND: Duration = Duration::from_secs(15);
/// Send a heartbeat after this much silence from the host.
const HEARTBEAT: Duration = Duration::from_secs(5);
/// Give up after this much silence from the app (three missed beats).
const SILENCE: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Conn(#[from] ConnError),
    #[error(
        "{app} is not paired with this computer. Run `forge device pair` with the phone \
         plugged in and the app open"
    )]
    NotPaired { app: String },
    #[error(
        "{app} is already paired with another computer (or with this one before its key \
         was deleted). To pair again, close the app, open it, and run `forge device pair` \
         within two minutes"
    )]
    PairingClosed { app: String },
    #[error(
        "{app} refused this computer's pairing key (it was paired again, or unpaired). \
         Run `forge device pair` with the app open"
    )]
    KeyRejected { app: String },
    #[error("the app answered: {0}")]
    App(String),
    #[error("unexpected message from the app: {0}")]
    Unexpected(String),
    #[error("the app ended the session: {0}")]
    Ended(Bye),
}

/// Why a session ended, in words for the CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bye(pub ByeReason);

impl std::fmt::Display for Bye {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            ByeReason::Background => "the app went to the background (suspended)",
            ByeReason::Stopped => "the app stopped its development server",
            ByeReason::Unpaired => "the pairing was removed",
            ByeReason::Replaced => "another computer connected",
        })
    }
}

pub type Result<T> = std::result::Result<T, ClientError>;

/// Reads the app's preamble: which app, and whether it is paired.
fn preamble<S: Read + Write>(conn: &mut Conn<S>) -> Result<Preamble> {
    let bytes = conn.recv_clear_bytes(STEP)?;
    Ok(Preamble::parse(&bytes).map_err(ConnError::from)?)
}

/// Pairs with the app on `stream` using `secret`. Returns the app's
/// preamble, so the caller can store the secret under the app's name.
pub fn pair<S: Read + Write>(stream: S, secret: &Secret) -> Result<Preamble> {
    let mut conn = Conn::new(stream);
    let pre = preamble(&mut conn)?;
    if !pre.pairing_open {
        return Err(ClientError::PairingClosed { app: pre.app });
    }
    conn.send_clear(&Open::Pair)?;
    conn.handshake(Role::Host, Purpose::Pair, STEP)?;
    conn.send(&HostMsg::Pair {
        secret: secret.to_vec(),
    })?;
    match conn.recv_within::<AppMsg>(STEP)? {
        AppMsg::Paired => Ok(pre),
        other => Err(ClientError::Unexpected(format!("{other:?}"))),
    }
}

/// Something the app streamed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Record {
        seq: u64,
        record: Record,
    },
    /// Records the app dropped before this host could see them.
    Gap {
        missed: u64,
    },
}

/// An open session.
pub struct Client<S> {
    conn: Conn<S>,
    pub preamble: Preamble,
    pub hello: Hello,
    next_id: u64,
    last_seq: u64,
    events: VecDeque<Event>,
    last_sent: Instant,
    last_heard: Instant,
}

/// Where a session's record stream starts.
#[derive(Clone, Copy, Debug)]
pub enum From {
    /// Everything after this sequence number (a reconnect).
    After(u64),
    /// Only the newest `n` buffered records (a first attach).
    Tail(u32),
}

impl<S: Read + Write> Client<S> {
    /// Opens a session. `secret_for` looks up the stored secret for the app
    /// named in the preamble.
    pub fn open(
        stream: S,
        secret_for: impl FnOnce(&Preamble) -> Option<Secret>,
        from: From,
    ) -> Result<Self> {
        let mut conn = Conn::new(stream);
        let pre = preamble(&mut conn)?;
        let not_paired = || ClientError::NotPaired {
            app: pre.app.clone(),
        };
        if !pre.paired {
            return Err(not_paired());
        }
        let secret = secret_for(&pre).ok_or_else(not_paired)?;
        conn.send_clear(&Open::Session)?;
        let rejected = || ClientError::KeyRejected {
            app: pre.app.clone(),
        };
        // A wrong key fails the handshake on the app, which hangs up.
        conn.handshake(Role::Host, Purpose::Session(&secret), STEP)
            .map_err(|e| match e {
                ConnError::Proto(crate::ProtoError::HandshakeFailed) | ConnError::Closed => {
                    rejected()
                }
                e => e.into(),
            })?;
        let (ack, tail) = match from {
            From::After(ack) => (ack, None),
            From::Tail(n) => (0, Some(n)),
        };
        let attached = conn.send(&HostMsg::Attach { ack, tail });
        let hello = match attached.and_then(|_| conn.recv_within::<AppMsg>(STEP)) {
            Err(ConnError::Closed) => return Err(rejected()),
            Err(e) => return Err(e.into()),
            Ok(AppMsg::Hello(hello)) => hello,
            Ok(AppMsg::Bye { reason }) => return Err(ClientError::Ended(Bye(reason))),
            Ok(other) => return Err(ClientError::Unexpected(format!("{other:?}"))),
        };
        let now = Instant::now();
        Ok(Self {
            conn,
            preamble: pre,
            last_seq: ack,
            hello,
            next_id: 1,
            events: VecDeque::new(),
            last_sent: now,
            last_heard: now,
        })
    }

    /// The last record sequence number received; pass it to
    /// [`From::After`] when reconnecting.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    fn send(&mut self, msg: &HostMsg) -> Result<()> {
        self.conn.send(msg)?;
        self.last_sent = Instant::now();
        Ok(())
    }

    /// Reads one message if one arrives within the stream's read timeout.
    /// Records and gaps are queued; returns other messages.
    fn pump(&mut self) -> Result<Option<AppMsg>> {
        if self.last_sent.elapsed() >= HEARTBEAT {
            self.send(&HostMsg::Heartbeat)?;
        }
        let msg = match self.conn.recv::<AppMsg>() {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                if self.last_heard.elapsed() >= SILENCE {
                    return Err(ConnError::Timeout(SILENCE).into());
                }
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        self.last_heard = Instant::now();
        match msg {
            AppMsg::Record { seq, record } => {
                self.last_seq = seq;
                self.events.push_back(Event::Record { seq, record });
                Ok(None)
            }
            AppMsg::Gap { missed } => {
                self.events.push_back(Event::Gap { missed });
                Ok(None)
            }
            AppMsg::Heartbeat => Ok(None),
            AppMsg::Bye { reason } => Err(ClientError::Ended(Bye(reason))),
            other => Ok(Some(other)),
        }
    }

    /// Runs a command and waits for its reply. Records that arrive in the
    /// meantime are kept for [`Client::next_event`].
    pub fn command(&mut self, command: Command) -> Result<Reply> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&HostMsg::Command { id, command })?;
        let deadline = Instant::now() + COMMAND;
        loop {
            if let Some(msg) = self.pump()? {
                match msg {
                    AppMsg::Reply { id: got, result } if got == id => {
                        return result.map_err(ClientError::App)
                    }
                    other => return Err(ClientError::Unexpected(format!("{other:?}"))),
                }
            }
            if Instant::now() >= deadline {
                return Err(ConnError::Timeout(COMMAND).into());
            }
        }
    }

    /// The next record or gap, or `None` if nothing arrived within the
    /// stream's read timeout. Keeps the session alive with heartbeats.
    pub fn next_event(&mut self) -> Result<Option<Event>> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        if let Some(other) = self.pump()? {
            return Err(ClientError::Unexpected(format!("{other:?}")));
        }
        Ok(self.events.pop_front())
    }
}

/// Parses a stored secret (hex).
pub fn parse_secret(hex: &str) -> Option<Secret> {
    let hex = hex.trim();
    if hex.len() != SECRET_LEN * 2 {
        return None;
    }
    let mut secret = [0u8; SECRET_LEN];
    for (i, byte) in secret.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(secret)
}

/// A secret as hex, for storing.
pub fn format_secret(secret: &Secret) -> String {
    secret.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_round_trip_as_hex() {
        let secret = crate::new_secret();
        assert_eq!(parse_secret(&format_secret(&secret)), Some(secret));
        assert_eq!(parse_secret("abc"), None);
        assert_eq!(parse_secret(&"zz".repeat(32)), None);
    }
}
