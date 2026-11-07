//! The messages, both directions. Every connection goes:
//!
//! 1. app → host, cleartext: [`Preamble`] (which app, which protocol).
//! 2. host → app, cleartext: [`Open`] (pair, or open a session).
//! 3. The Noise handshake ([`crate::secure`]).
//! 4. Encrypted [`HostMsg`] and [`AppMsg`] until either side hangs up.
//!
//! Pairing: the host sends [`HostMsg::Pair`], the app stores the secret and
//! answers [`AppMsg::Paired`], and the connection ends.
//!
//! A session: the host sends [`HostMsg::Attach`] with the last record it saw;
//! the app answers [`AppMsg::Hello`], replays what the host missed, then
//! streams new [`AppMsg::Record`]s, answers [`HostMsg::Command`]s, and sends a
//! [`AppMsg::Heartbeat`] when it has been quiet. When the app leaves the
//! screen it says [`AppMsg::Bye`] with [`ByeReason::Background`] and closes;
//! iOS suspends it soon after.

use serde::{Deserialize, Serialize};

use crate::{ProtoError, Result, MAGIC, PROTOCOL};

/// The first frame on every connection, in cleartext, from the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preamble {
    pub magic: String,
    pub protocol: u16,
    /// The app's identity, normally its bundle ID. The host keys its stored
    /// secret by device and app.
    pub app: String,
    /// Whether the app holds a pairing secret.
    pub paired: bool,
    /// Whether the app accepts a new pairing now (it has none, or it started
    /// moments ago, so whoever pairs could also relaunch it).
    pub pairing_open: bool,
}

impl Preamble {
    pub fn new(app: &str, paired: bool, pairing_open: bool) -> Self {
        Self {
            magic: MAGIC.into(),
            protocol: PROTOCOL,
            app: app.into(),
            paired,
            pairing_open,
        }
    }

    /// Decodes and checks a preamble: right service, same protocol.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // Decode loosely first so that a future preamble with more fields,
        // or some other service entirely, gets a message in words.
        #[derive(Deserialize)]
        struct Probe {
            magic: Option<String>,
            protocol: Option<u16>,
        }
        let probe: Probe = crate::decode(bytes)
            .map_err(|_| ProtoError::NotForge(String::from_utf8_lossy(bytes).into_owned()))?;
        let magic = probe.magic.unwrap_or_default();
        if magic != MAGIC {
            return Err(ProtoError::NotForge(magic));
        }
        let protocol = probe.protocol.unwrap_or_default();
        if protocol != PROTOCOL {
            return Err(ProtoError::Version {
                app: protocol,
                host: PROTOCOL,
            });
        }
        crate::decode(bytes)
    }
}

/// The host's answer to the preamble, in cleartext.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Open {
    Pair,
    Session,
}

/// Host to app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostMsg {
    /// Starts a session. `ack` is the last record sequence number this host
    /// saw from this app (0 for none). With `ack` 0, `tail` limits the replay
    /// to the newest records.
    Attach {
        ack: u64,
        tail: Option<u32>,
    },
    Command {
        id: u64,
        command: Command,
    },
    /// Only on a pairing connection: the secret to store.
    Pair {
        secret: Vec<u8>,
    },
    Heartbeat,
}

/// App to host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppMsg {
    Hello(Hello),
    Record {
        seq: u64,
        record: Record,
    },
    /// `missed` records were dropped from the buffer before this host could
    /// see them.
    Gap {
        missed: u64,
    },
    Reply {
        id: u64,
        result: std::result::Result<Reply, String>,
    },
    Paired,
    Heartbeat,
    Bye {
        reason: ByeReason,
    },
}

/// Why the app closed the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByeReason {
    /// The app left the screen. It comes back when the user opens it.
    Background,
    /// The app shut its development server down.
    Stopped,
    /// The host asked to unpair.
    Unpaired,
    /// Another host connected.
    Replaced,
}

/// What the app says when a session opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub app: String,
    /// The build the app was compiled from (version, and commit when known).
    pub build: String,
    pub phase: String,
    /// When the app's development server started (Unix ms).
    pub started_ms: i64,
    /// The sequence number the next new record will get.
    pub next_seq: u64,
}

/// One item of the app's live record stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Record {
    Log(LogLine),
    /// The app's lifecycle phase changed.
    Phase {
        phase: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// Unix time in milliseconds.
    pub time_ms: i64,
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub level: String,
    pub target: String,
    pub message: String,
}

/// What a host can ask. Closed: an app extends it only through
/// [`Command::App`], with commands it registered itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Ping,
    Status,
    /// The values of every state probe the app registered.
    Snapshot,
    /// The app's own commands.
    ListCommands,
    /// Forget the pairing on the app; the session ends after the reply.
    Unpair,
    /// One of the app's registered commands.
    App {
        name: String,
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Pong,
    Status(Status),
    Snapshot(Vec<Probe>),
    Commands(Vec<CommandInfo>),
    /// An app command's output.
    Done(String),
    Unpaired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub app: String,
    pub build: String,
    pub phase: String,
    pub uptime_ms: u64,
    /// Records in the replay buffer, and the sequence numbers they span.
    pub buffered: u64,
    pub oldest_seq: u64,
    pub next_seq: u64,
    pub probes: Vec<String>,
    pub commands: Vec<String>,
}

/// One named value the app chose to expose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub help: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let msgs = [
            AppMsg::Hello(Hello {
                app: "org.example.chat".into(),
                build: "0.1.0".into(),
                phase: "Active".into(),
                started_ms: 1,
                next_seq: 7,
            }),
            AppMsg::Record {
                seq: 3,
                record: Record::Log(LogLine {
                    time_ms: 5,
                    level: "INFO".into(),
                    target: "chat".into(),
                    message: "sent".into(),
                }),
            },
            AppMsg::Reply {
                id: 1,
                result: Err("no such command".into()),
            },
            AppMsg::Bye {
                reason: ByeReason::Background,
            },
        ];
        for msg in msgs {
            assert_eq!(decode::<AppMsg>(&encode(&msg)).unwrap(), msg);
        }
        let cmd = HostMsg::Command {
            id: 2,
            command: Command::App {
                name: "reset".into(),
                args: vec!["outbox".into()],
            },
        };
        assert_eq!(decode::<HostMsg>(&encode(&cmd)).unwrap(), cmd);
    }

    #[test]
    fn preamble_mismatches_are_explained() {
        let ok = Preamble::new("org.example.app", true, false);
        assert_eq!(Preamble::parse(&encode(&ok)).unwrap(), ok);

        let mut newer = ok.clone();
        newer.protocol = PROTOCOL + 1;
        let e = Preamble::parse(&encode(&newer)).unwrap_err();
        assert!(matches!(e, ProtoError::Version { .. }), "{e}");
        assert!(e.to_string().contains("rebuild the app"), "{e}");

        let e = Preamble::parse(b"SSH-2.0-OpenSSH\r\n").unwrap_err();
        assert!(matches!(e, ProtoError::NotForge(_)), "{e}");
    }
}
