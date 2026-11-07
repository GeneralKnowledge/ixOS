//! The wire protocol of the Forge Device live connection (M1): what a
//! development build of an app and `forge device` say to each other.
//!
//! This crate does no I/O, so both ends share it and every rule is tested on
//! Linux. Bottom up:
//!
//! 1. [`frame`]: length-prefixed frames with a hard size cap, so a bad peer
//!    cannot make either side allocate without bound.
//! 2. [`Preamble`] and [`Open`]: two cleartext frames that say which app is
//!    listening, which protocol version it speaks and whether this connection
//!    pairs or opens a session. A version mismatch is reported in words here,
//!    before any decoding of later messages can fail.
//! 3. [`secure`]: a Noise handshake, then every frame encrypted. Sessions
//!    use `Noise_XXpsk3` keyed by the pairing secret, so only a paired host
//!    gets in. Pairing uses plain `Noise_XX`: it runs only over USB, where the
//!    phone's own "Trust This Computer" already vouches for the host.
//! 4. [`HostMsg`] and [`AppMsg`]: the messages, CBOR-encoded.
//! 5. [`Outbox`]: the app's numbered record buffer. A host that reconnects
//!    (after the app was in the background, say) sends the last sequence
//!    number it saw and gets everything after it, or a gap count when the
//!    buffer has moved on.
//!
//! Commands are a closed set ([`Command`]); apps add named commands with
//! string arguments, and nothing can run code the app did not register.

pub mod client;
pub mod conn;
pub mod frame;
mod message;
mod outbox;
pub mod secure;

pub use message::*;
pub use outbox::Outbox;

/// The protocol version this crate speaks. Bump it on any change an older
/// peer would misread; the preamble carries it so a mismatch is explained.
pub const PROTOCOL: u16 = 1;

/// The TCP port a development build listens on, on the phone's loopback
/// interface only. The host reaches it through usbmuxd.
pub const DEFAULT_PORT: u16 = 47_811;

/// Identifies the preamble, so a host that reaches some other service on the
/// port says so instead of failing to decode.
pub const MAGIC: &str = "forge-devtools";

/// Length of the pairing secret, in bytes.
pub const SECRET_LEN: usize = 32;

/// The pairing secret both ends store.
pub type Secret = [u8; SECRET_LEN];

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("frame of {0} bytes is over the {max} byte limit", max = frame::MAX_FRAME)]
    FrameTooLarge(usize),
    #[error("not a Forge development build on this port (got {0:?})")]
    NotForge(String),
    #[error(
        "the app speaks devtools protocol {app} and this forge speaks {host}; \
         rebuild the app with this forge checkout (forge run ios --devtools)"
    )]
    Version { app: u16, host: u16 },
    #[error("could not decode a message: {0}")]
    Decode(String),
    #[error("encryption failed: {0}")]
    Crypto(String),
    #[error("the handshake failed: the app and this computer are not paired with the same secret")]
    HandshakeFailed,
}

pub type Result<T> = std::result::Result<T, ProtoError>;

/// CBOR-encodes a value.
pub fn encode<T: serde::Serialize>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    // Writing into a Vec cannot fail and our types always serialize.
    ciborium::into_writer(value, &mut out).expect("CBOR encoding into a Vec");
    out
}

/// Decodes a CBOR value.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    ciborium::from_reader(bytes).map_err(|e| ProtoError::Decode(e.to_string()))
}

/// A fresh random pairing secret.
pub fn new_secret() -> Secret {
    // A Curve25519 private key is 32 bytes from the system random source.
    let keypair = secure::keypair();
    let mut secret = [0u8; SECRET_LEN];
    secret.copy_from_slice(&keypair);
    secret
}
