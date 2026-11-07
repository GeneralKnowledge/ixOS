//! Frames and the encrypted channel over any blocking byte stream. Both ends
//! use it: the app over a loopback TCP socket, the host over a usbmuxd
//! connection, tests over an in-memory pipe.
//!
//! The stream should have a short read timeout. [`Conn::recv`] then returns
//! `Ok(None)` when nothing arrived in time, so a loop can do other work
//! (send new records, a heartbeat) between reads.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use serde::{de::DeserializeOwned, Serialize};

use crate::frame::{self, FrameReader};
use crate::secure::{Channel, Handshake, Purpose, Role};
use crate::{decode, encode, ProtoError};

#[derive(Debug, thiserror::Error)]
pub enum ConnError {
    #[error("{0}")]
    Proto(#[from] ProtoError),
    #[error("connection error: {0}")]
    Io(#[from] io::Error),
    #[error("the other side closed the connection")]
    Closed,
    #[error("no answer within {0:?}")]
    Timeout(Duration),
}

/// A connection, in cleartext until [`Conn::handshake`].
pub struct Conn<S> {
    stream: S,
    frames: FrameReader,
    channel: Option<Channel>,
}

fn quiet(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

impl<S: Read + Write> Conn<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            frames: FrameReader::new(),
            channel: None,
        }
    }

    pub fn stream(&self) -> &S {
        &self.stream
    }

    fn send_frame(&mut self, payload: &[u8]) -> Result<(), ConnError> {
        let bytes = frame::encode(payload)?;
        self.stream.write_all(&bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    /// The next frame, or `None` if none arrived within the stream's read
    /// timeout.
    fn recv_frame(&mut self) -> Result<Option<Vec<u8>>, ConnError> {
        loop {
            if let Some(frame) = self.frames.next_frame()? {
                return Ok(Some(frame));
            }
            let mut buf = [0u8; 16 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(ConnError::Closed),
                Ok(n) => self.frames.push(&buf[..n]),
                Err(e) if quiet(&e) => return Ok(None),
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Err(ConnError::Closed)
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Waits up to `limit` for a frame.
    fn recv_frame_within(&mut self, limit: Duration) -> Result<Vec<u8>, ConnError> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(frame) = self.recv_frame()? {
                return Ok(frame);
            }
            if Instant::now() >= deadline {
                return Err(ConnError::Timeout(limit));
            }
        }
    }

    /// Sends a cleartext message (the preamble and [`crate::Open`] only).
    pub fn send_clear<T: Serialize>(&mut self, msg: &T) -> Result<(), ConnError> {
        self.send_frame(&encode(msg))
    }

    /// Receives a cleartext frame within `limit`.
    pub fn recv_clear_bytes(&mut self, limit: Duration) -> Result<Vec<u8>, ConnError> {
        self.recv_frame_within(limit)
    }

    /// Runs the Noise handshake; afterwards every message is encrypted.
    pub fn handshake(
        &mut self,
        role: Role,
        purpose: Purpose<'_>,
        limit: Duration,
    ) -> Result<(), ConnError> {
        let mut hs = Handshake::new(role, purpose)?;
        while !hs.finished() {
            if hs.my_turn() {
                let msg = hs.write()?;
                self.send_frame(&msg)?;
            } else {
                match self.recv_frame_within(limit) {
                    Ok(msg) => hs.read(&msg)?,
                    // The app hangs up when the host's proof is wrong.
                    Err(ConnError::Closed) => return Err(ProtoError::HandshakeFailed.into()),
                    Err(e) => return Err(e),
                }
            }
        }
        self.channel = Some(hs.into_channel()?);
        Ok(())
    }

    fn channel(&mut self) -> &mut Channel {
        self.channel
            .as_mut()
            .expect("encrypted messages are only sent after the handshake")
    }

    /// Sends an encrypted message.
    pub fn send<T: Serialize>(&mut self, msg: &T) -> Result<(), ConnError> {
        let sealed = self.channel().seal(&encode(msg))?;
        self.send_frame(&sealed)
    }

    /// The next encrypted message, or `None` if nothing arrived within the
    /// stream's read timeout.
    pub fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, ConnError> {
        let Some(frame) = self.recv_frame()? else {
            return Ok(None);
        };
        let plain = self.channel().open(&frame)?;
        Ok(Some(decode(&plain)?))
    }

    /// Waits up to `limit` for the next encrypted message.
    pub fn recv_within<T: DeserializeOwned>(&mut self, limit: Duration) -> Result<T, ConnError> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(msg) = self.recv()? {
                return Ok(msg);
            }
            if Instant::now() >= deadline {
                return Err(ConnError::Timeout(limit));
            }
        }
    }
}
