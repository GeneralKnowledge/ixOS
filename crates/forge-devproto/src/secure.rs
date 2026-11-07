//! The Noise handshake and the encrypted channel after it.
//!
//! The host is always the initiator. Both ends use a fresh static key per
//! connection: who is who is proven by the pairing secret (the `psk3`
//! token), not by remembered public keys, so there is one secret to store,
//! rotate and delete.

use snow::{Builder, HandshakeState, TransportState};

use crate::{frame::MAX_FRAME, ProtoError, Result, Secret};

/// Sessions: mutual authentication by the pairing secret.
const SESSION: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
/// Pairing over USB: encrypted, authenticated by the USB path itself.
const PAIRING: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Noise's authentication tag on every transport message.
const TAG: usize = 16;

/// The largest plaintext one encrypted frame can carry.
pub const MAX_PLAINTEXT: usize = MAX_FRAME - TAG;

/// Which end of the connection this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// `forge device` on the computer: speaks first.
    Host,
    /// The development build on the phone.
    App,
}

/// What the handshake is for.
#[derive(Clone, Copy)]
pub enum Purpose<'a> {
    /// First contact over USB, before there is a secret.
    Pair,
    /// Everything else, keyed by the secret both ends stored when pairing.
    Session(&'a Secret),
}

fn crypto(e: snow::Error) -> ProtoError {
    ProtoError::Crypto(e.to_string())
}

/// A new random Curve25519 private key.
pub(crate) fn keypair() -> Vec<u8> {
    Builder::new(SESSION.parse().expect("valid Noise pattern"))
        .generate_keypair()
        .expect("system random source")
        .private
}

/// A handshake in progress. Alternate [`Handshake::write`] and
/// [`Handshake::read`] as [`Handshake::my_turn`] says until
/// [`Handshake::finished`], then [`Handshake::into_channel`].
pub struct Handshake {
    state: HandshakeState,
}

impl Handshake {
    pub fn new(role: Role, purpose: Purpose<'_>) -> Result<Self> {
        let pattern = match purpose {
            Purpose::Pair => PAIRING,
            Purpose::Session(_) => SESSION,
        };
        let key = keypair();
        let mut builder = Builder::new(pattern.parse().expect("valid Noise pattern"))
            .local_private_key(&key)
            .map_err(crypto)?;
        if let Purpose::Session(secret) = purpose {
            builder = builder.psk(3, secret).map_err(crypto)?;
        }
        let state = match role {
            Role::Host => builder.build_initiator(),
            Role::App => builder.build_responder(),
        }
        .map_err(crypto)?;
        Ok(Self { state })
    }

    pub fn my_turn(&self) -> bool {
        self.state.is_my_turn()
    }

    pub fn finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// The next handshake message to send.
    pub fn write(&mut self) -> Result<Vec<u8>> {
        let mut out = vec![0u8; MAX_FRAME];
        let len = self.state.write_message(&[], &mut out).map_err(crypto)?;
        out.truncate(len);
        Ok(out)
    }

    /// Takes the peer's handshake message. A wrong secret fails here (on the
    /// app for the host's last message, on the host when the app hangs up).
    pub fn read(&mut self, message: &[u8]) -> Result<()> {
        let mut payload = vec![0u8; MAX_FRAME];
        self.state
            .read_message(message, &mut payload)
            .map(|_| ())
            .map_err(|_| ProtoError::HandshakeFailed)
    }

    pub fn into_channel(self) -> Result<Channel> {
        Ok(Channel {
            state: self.state.into_transport_mode().map_err(crypto)?,
        })
    }
}

/// Encrypts and decrypts frames after the handshake.
pub struct Channel {
    state: TransportState,
}

impl Channel {
    /// `plain` encrypted, ready to frame. At most [`MAX_PLAINTEXT`] bytes.
    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        if plain.len() > MAX_PLAINTEXT {
            return Err(ProtoError::FrameTooLarge(plain.len() + TAG));
        }
        let mut out = vec![0u8; plain.len() + TAG];
        let len = self.state.write_message(plain, &mut out).map_err(crypto)?;
        out.truncate(len);
        Ok(out)
    }

    /// Decrypts one frame. Fails on any tampering or reordering.
    pub fn open(&mut self, sealed: &[u8]) -> Result<Vec<u8>> {
        let mut out = vec![0u8; sealed.len()];
        let len = self.state.read_message(sealed, &mut out).map_err(crypto)?;
        out.truncate(len);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a handshake in memory; `None` if it failed.
    fn shake(host: Purpose<'_>, app: Purpose<'_>) -> Option<(Channel, Channel)> {
        let mut h = Handshake::new(Role::Host, host).unwrap();
        let mut a = Handshake::new(Role::App, app).unwrap();
        while !(h.finished() && a.finished()) {
            if h.my_turn() {
                a.read(&h.write().unwrap()).ok()?;
            } else {
                h.read(&a.write().unwrap()).ok()?;
            }
        }
        Some((h.into_channel().unwrap(), a.into_channel().unwrap()))
    }

    #[test]
    fn the_same_secret_opens_a_channel_both_ways() {
        let secret = crate::new_secret();
        let (mut host, mut app) =
            shake(Purpose::Session(&secret), Purpose::Session(&secret)).unwrap();
        let sealed = host.seal(b"status").unwrap();
        assert_ne!(sealed, b"status");
        assert_eq!(app.open(&sealed).unwrap(), b"status");
        assert_eq!(host.open(&app.seal(b"ok").unwrap()).unwrap(), b"ok");
    }

    #[test]
    fn a_different_secret_is_refused() {
        let (a, b) = (crate::new_secret(), crate::new_secret());
        assert_ne!(a, b);
        assert!(shake(Purpose::Session(&a), Purpose::Session(&b)).is_none());
    }

    #[test]
    fn pairing_and_session_handshakes_do_not_mix() {
        let secret = crate::new_secret();
        assert!(shake(Purpose::Pair, Purpose::Session(&secret)).is_none());
        assert!(shake(Purpose::Pair, Purpose::Pair).is_some());
    }

    #[test]
    fn tampered_or_replayed_frames_fail() {
        let secret = crate::new_secret();
        let (mut host, mut app) =
            shake(Purpose::Session(&secret), Purpose::Session(&secret)).unwrap();
        let mut tampered = host.seal(b"cmd").unwrap();
        tampered[0] ^= 1;
        assert!(app.open(&tampered).is_err());

        let (mut host, mut app) =
            shake(Purpose::Session(&secret), Purpose::Session(&secret)).unwrap();
        let first = host.seal(b"one").unwrap();
        app.open(&first).unwrap();
        assert!(app.open(&first).is_err(), "a replayed frame must fail");
    }
}
