//! Sealed mailbox items (N3 step 1, examples/relay/docs/relay-mailbox.md).
//!
//! What a device leaves with the mailbox for another device, and how the
//! recipient opens it. Pure functions with no I/O, so every rule here is
//! tested on Linux and pinned by a golden fixture; the server (step 2) and
//! the outbox (step 3) build on them.
//!
//! - **The mailbox key** is an X25519 key of its own, not the endpoint key.
//!   A device tells it to trusted devices in an [`Announcement`] signed by
//!   its endpoint key, so the mailbox server never sees it.
//! - **A sealed item** is HPKE (RFC 9180, base mode, X25519 + HKDF-SHA256 +
//!   ChaCha20-Poly1305) to the recipient's mailbox key. Its header (`id`,
//!   `to`, `from`, `expires_ms`) is bound in as associated data, so the
//!   server can't move an item to someone else or stretch its life.
//! - **Inside**, the payload is signed by the sender's endpoint key, so
//!   the recipient knows who sent it even though the server delivered it.
//! - **Deposit passes** let only trusted devices leave items: the
//!   recipient hands each one a random pass and registers its SHA-256.
//!
//! Replay is the outbox's 45-day memory of ids; [`open`] returns the id
//! for it to check.

// Used by the mailbox client in step 3; until then only the tests call it.
#![cfg_attr(not(test), allow(dead_code))]

use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as _, OpModeR, OpModeS, Serializable};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

/// The wire version of a sealed item and an announcement.
pub(crate) const VERSION: u8 = 1;
/// HPKE `info`: binds every key schedule to this use.
const INFO: &[u8] = b"relay/mailbox/1 item";
/// What the sender's endpoint key signs inside an item.
const ITEM_SIGNED: &[u8] = b"relay/mailbox/1 signed item";
/// What a device's endpoint key signs in its announcement.
const ANNOUNCED: &[u8] = b"relay/mailbox/1 key";

type Kem = X25519HkdfSha256;

/// Why an item or announcement wasn't accepted. Each is a reason to drop
/// it, never to retry it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum MailboxError {
    #[error("not addressed to this device")]
    NotForUs,
    #[error("expired")]
    Expired,
    #[error("can't be opened with this device's mailbox key")]
    Unreadable,
    #[error("not signed by the device it says it's from")]
    BadSignature,
    #[error("from a device this one doesn't trust")]
    Untrusted,
    #[error("not a version this device reads")]
    UnknownVersion,
    #[error("malformed")]
    Malformed,
}

/// This device's mailbox key: a 32-byte seed the X25519 key pair is
/// derived from (RFC 9180 `DeriveKeyPair`), kept like the device key.
#[derive(Clone)]
pub(crate) struct MailboxKey {
    seed: [u8; 32],
}

impl MailboxKey {
    pub(crate) fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("the system's random source");
        Self { seed }
    }

    pub(crate) fn from_seed(seed: [u8; 32]) -> Self {
        Self { seed }
    }

    pub(crate) fn seed(&self) -> [u8; 32] {
        self.seed
    }

    fn pair(
        &self,
    ) -> (
        <Kem as hpke::Kem>::PrivateKey,
        <Kem as hpke::Kem>::PublicKey,
    ) {
        Kem::derive_keypair(&self.seed)
    }

    /// The public half, as trusted devices are told it.
    pub(crate) fn public(&self) -> [u8; 32] {
        self.pair().1.to_bytes().into()
    }

    /// The announcement trusted devices get in the Hello.
    pub(crate) fn announce(&self, device: &SecretKey, issued_ms: i64) -> Announcement {
        let key = self.public();
        let signature = device.sign(&announced(&device.public(), &key, issued_ms));
        Announcement {
            version: VERSION,
            key,
            issued_ms,
            signature: signature.to_bytes().to_vec(),
        }
    }
}

impl std::fmt::Debug for MailboxKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailboxKey(…)")
    }
}

/// A device's mailbox key, signed by its endpoint key: what a trusted
/// device seals items to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Announcement {
    pub version: u8,
    pub key: [u8; 32],
    /// When it was made: a newer one replaces an older one.
    pub issued_ms: i64,
    #[serde(with = "serde_bytes_vec")]
    pub signature: Vec<u8>,
}

impl Announcement {
    /// The mailbox key, if `device` signed this announcement.
    pub(crate) fn verify(&self, device: &PublicKey) -> Result<[u8; 32], MailboxError> {
        if self.version != VERSION {
            return Err(MailboxError::UnknownVersion);
        }
        let signature = signature(&self.signature)?;
        device
            .verify(&announced(device, &self.key, self.issued_ms), &signature)
            .map_err(|_| MailboxError::BadSignature)?;
        <Kem as hpke::Kem>::PublicKey::from_bytes(&self.key)
            .map_err(|_| MailboxError::Malformed)?;
        Ok(self.key)
    }
}

fn announced(device: &PublicKey, key: &[u8; 32], issued_ms: i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(ANNOUNCED.len() + 32 + 32 + 8);
    out.extend_from_slice(ANNOUNCED);
    out.extend_from_slice(device.as_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(&issued_ms.to_be_bytes());
    out
}

/// What the server sees of an item, and what the encryption is bound to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub version: u8,
    /// The outbox delivery id: the same one a direct delivery uses.
    pub id: [u8; 16],
    pub to: [u8; 32],
    pub from: [u8; 32],
    pub expires_ms: i64,
}

impl Header {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 16 + 32 + 32 + 8);
        out.push(self.version);
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&self.to);
        out.extend_from_slice(&self.from);
        out.extend_from_slice(&self.expires_ms.to_be_bytes());
        out
    }
}

/// One item as it is left with, and kept by, the mailbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SealedItem {
    pub header: Header,
    /// HPKE's encapsulated key.
    #[serde(with = "serde_bytes_vec")]
    pub enc: Vec<u8>,
    #[serde(with = "serde_bytes_vec")]
    pub ciphertext: Vec<u8>,
}

/// What is encrypted: the payload and the sender's signature over it.
#[derive(Serialize, Deserialize)]
struct Inner {
    #[serde(with = "serde_bytes_vec")]
    signature: Vec<u8>,
    #[serde(with = "serde_bytes_vec")]
    payload: Vec<u8>,
}

fn signed(header: &Header, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ITEM_SIGNED.len() + 89 + payload.len());
    out.extend_from_slice(ITEM_SIGNED);
    out.extend_from_slice(&header.bytes());
    out.extend_from_slice(payload);
    out
}

/// Seals `payload` (the outbox's item, encoded) for `to`, whose mailbox key
/// is `to_key` (from its verified [`Announcement`]).
pub(crate) fn seal(
    sender: &SecretKey,
    to: &PublicKey,
    to_key: &[u8; 32],
    id: [u8; 16],
    expires_ms: i64,
    payload: &[u8],
) -> Result<SealedItem, MailboxError> {
    let header = Header {
        version: VERSION,
        id,
        to: *to.as_bytes(),
        from: *sender.public().as_bytes(),
        expires_ms,
    };
    seal_as(header, sender, to_key, payload)
}

/// Seals `payload` under `header`, signed by `signer`: [`seal`], and in
/// tests a signer that isn't the header's `from`.
fn seal_as(
    header: Header,
    signer: &SecretKey,
    to_key: &[u8; 32],
    payload: &[u8],
) -> Result<SealedItem, MailboxError> {
    let signature = signer.sign(&signed(&header, payload));
    let inner = Inner {
        signature: signature.to_bytes().to_vec(),
        payload: payload.to_vec(),
    };
    let mut plaintext = Vec::new();
    ciborium::into_writer(&inner, &mut plaintext).map_err(|_| MailboxError::Malformed)?;
    let recipient =
        <Kem as hpke::Kem>::PublicKey::from_bytes(to_key).map_err(|_| MailboxError::Malformed)?;
    let (enc, ciphertext) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeS::Base,
        &recipient,
        INFO,
        &plaintext,
        &header.bytes(),
    )
    .map_err(|_| MailboxError::Malformed)?;
    Ok(SealedItem {
        header,
        enc: enc.to_bytes().to_vec(),
        ciphertext,
    })
}

/// An item this device opened: who sent it, its id, and the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opened {
    pub from: PublicKey,
    pub id: [u8; 16],
    pub payload: Vec<u8>,
}

/// Opens an item addressed to `me` with this device's mailbox key. It is
/// accepted only if it hasn't expired at `now_ms`, opens with the key, is
/// signed by the device in its header, and `trusted` says that device is
/// trusted here.
pub(crate) fn open(
    key: &MailboxKey,
    me: &PublicKey,
    item: &SealedItem,
    now_ms: i64,
    trusted: impl Fn(&PublicKey) -> bool,
) -> Result<Opened, MailboxError> {
    let header = &item.header;
    if header.version != VERSION {
        return Err(MailboxError::UnknownVersion);
    }
    if header.to != *me.as_bytes() {
        return Err(MailboxError::NotForUs);
    }
    if header.expires_ms <= now_ms {
        return Err(MailboxError::Expired);
    }
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&item.enc)
        .map_err(|_| MailboxError::Malformed)?;
    let plaintext = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        &key.pair().0,
        &enc,
        INFO,
        &item.ciphertext,
        &header.bytes(),
    )
    .map_err(|_| MailboxError::Unreadable)?;
    let inner: Inner =
        ciborium::from_reader(plaintext.as_slice()).map_err(|_| MailboxError::Malformed)?;
    let from = PublicKey::from_bytes(&header.from).map_err(|_| MailboxError::Malformed)?;
    from.verify(
        &signed(header, &inner.payload),
        &signature(&inner.signature)?,
    )
    .map_err(|_| MailboxError::BadSignature)?;
    if !trusted(&from) {
        return Err(MailboxError::Untrusted);
    }
    Ok(Opened {
        from,
        id: header.id,
        payload: inner.payload,
    })
}

fn signature(bytes: &[u8]) -> Result<Signature, MailboxError> {
    let bytes: &[u8; Signature::LENGTH] = bytes.try_into().map_err(|_| MailboxError::Malformed)?;
    Ok(Signature::from_bytes(bytes))
}

/// A deposit pass: what a recipient gives one trusted device so it may
/// leave items. The server keeps only [`DepositPass::digest`].
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DepositPass([u8; 32]);

impl DepositPass {
    pub(crate) fn generate() -> Self {
        let mut pass = [0u8; 32];
        getrandom::fill(&mut pass).expect("the system's random source");
        Self(pass)
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) fn bytes(&self) -> [u8; 32] {
        self.0
    }

    /// What the recipient registers with the mailbox.
    pub(crate) fn digest(&self) -> [u8; 32] {
        use sha2::Digest as _;
        sha2::Sha256::digest(self.0).into()
    }
}

impl std::fmt::Debug for DepositPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DepositPass(…)")
    }
}

/// Byte strings as CBOR byte strings, not arrays of numbers.
pub(crate) mod serde_bytes_vec {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let buf = ciborium::Value::deserialize(d)?;
        match buf {
            ciborium::Value::Bytes(b) => Ok(b),
            _ => Err(serde::de::Error::custom("expected bytes")),
        }
    }
}

/// Where the mailbox's own secrets come from: both are derived from the
/// device key (HKDF-SHA256), so there is no second secret to keep, back up
/// or lose. Losing the device key loses them too, as it loses everything.
const DERIVED: &[u8] = b"relay/mailbox/1 derived";

fn derive(device: &SecretKey, label: &[u8], peer: Option<&PublicKey>) -> [u8; 32] {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(DERIVED), &device.to_bytes());
    let mut info = label.to_vec();
    if let Some(peer) = peer {
        info.extend_from_slice(peer.as_bytes());
    }
    let mut out = [0u8; 32];
    hk.expand(&info, &mut out)
        .expect("32 bytes is a valid HKDF-SHA256 length");
    out
}

impl MailboxKey {
    /// This device's mailbox key, the same every time from its device key.
    pub(crate) fn of_device(device: &SecretKey) -> Self {
        Self::from_seed(derive(device, b"mailbox key", None))
    }
}

impl DepositPass {
    /// The pass this device gives `peer`: the same every time, and
    /// different for every peer, so revoking one deregisters only its own.
    pub(crate) fn for_peer(device: &SecretKey, peer: &PublicKey) -> Self {
        Self(derive(device, b"deposit pass", Some(peer)))
    }
}

/// What a device tells a trusted device in the Hello so it can leave
/// items in its mailbox (N3 step 3): its mailbox key, signed; the pass
/// for that device alone; and the mailbox server it collects from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct MailboxIntro {
    pub announcement: Announcement,
    pub pass: [u8; 32],
    /// The mailbox server's endpoint id.
    pub server: [u8; 32],
}

impl MailboxIntro {
    /// This device's intro for `peer`, collecting at `server`.
    pub(crate) fn new(
        device: &SecretKey,
        peer: &PublicKey,
        server: &PublicKey,
        now_ms: i64,
    ) -> Self {
        Self {
            announcement: MailboxKey::of_device(device).announce(device, now_ms),
            pass: DepositPass::for_peer(device, peer).bytes(),
            server: *server.as_bytes(),
        }
    }

    /// What `device` told this one, if it signed it: what to keep.
    pub(crate) fn verify(&self, device: &PublicKey) -> Result<PeerMailbox, MailboxError> {
        let key = self.announcement.verify(device)?;
        let server = PublicKey::from_bytes(&self.server).map_err(|_| MailboxError::Malformed)?;
        Ok(PeerMailbox {
            key: crate::hex::hex(&key),
            issued_ms: self.announcement.issued_ms,
            pass: crate::hex::hex(&self.pass),
            server: server.to_string(),
        })
    }
}

/// A trusted device's mailbox, as this device keeps it (on
/// `SavedDevice`): where to leave items for it, sealed to what, under
/// which pass. Hex, as the record is JSON.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PeerMailbox {
    pub key: String,
    pub issued_ms: i64,
    pub pass: String,
    /// The mailbox server's endpoint id.
    pub server: String,
}

impl PeerMailbox {
    /// Whether `newer` says anything this one doesn't: a newer
    /// announcement of a different key, pass or server. Only then is the
    /// record written.
    pub(crate) fn replaced_by(&self, newer: &PeerMailbox) -> bool {
        newer.issued_ms > self.issued_ms
            && (newer.key != self.key || newer.pass != self.pass || newer.server != self.server)
    }

    pub(crate) fn key(&self) -> Option<[u8; 32]> {
        crate::hex::unhex(&self.key)?.try_into().ok()
    }

    pub(crate) fn pass(&self) -> Option<DepositPass> {
        let bytes: [u8; 32] = crate::hex::unhex(&self.pass)?.try_into().ok()?;
        Some(DepositPass::from_bytes(bytes))
    }

    pub(crate) fn server(&self) -> Option<PublicKey> {
        self.server.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const LATER: i64 = NOW + 7 * 24 * 60 * 60 * 1000;

    fn device(n: u8) -> SecretKey {
        SecretKey::from_bytes(&[n; 32])
    }

    fn sealed(from: &SecretKey, to: &SecretKey, key: &MailboxKey) -> SealedItem {
        seal(from, &to.public(), &key.public(), [7; 16], LATER, b"hello").unwrap()
    }

    #[test]
    fn a_trusted_device_opens_what_it_was_sent() {
        let (laptop, phone) = (device(1), device(2));
        let key = MailboxKey::from_seed([9; 32]);
        let item = sealed(&laptop, &phone, &key);
        let opened = open(&key, &phone.public(), &item, NOW, |_| true).unwrap();
        assert_eq!(opened.from, laptop.public());
        assert_eq!(opened.id, [7; 16]);
        assert_eq!(opened.payload, b"hello");
    }

    #[test]
    fn what_the_server_keeps_holds_no_plaintext() {
        let key = MailboxKey::from_seed([9; 32]);
        let item = seal(
            &device(1),
            &device(2).public(),
            &key.public(),
            [7; 16],
            LATER,
            b"a very recognisable message",
        )
        .unwrap();
        let mut stored = Vec::new();
        ciborium::into_writer(&item, &mut stored).unwrap();
        let needle = b"recognisable";
        assert!(!stored.windows(needle.len()).any(|w| w == needle));
    }

    #[test]
    fn another_devices_key_cant_open_it() {
        let phone = device(2);
        let item = sealed(&device(1), &phone, &MailboxKey::from_seed([9; 32]));
        let other = MailboxKey::from_seed([8; 32]);
        assert_eq!(
            open(&other, &phone.public(), &item, NOW, |_| true),
            Err(MailboxError::Unreadable)
        );
    }

    #[test]
    fn it_is_refused_when_not_addressed_here() {
        let key = MailboxKey::from_seed([9; 32]);
        let item = sealed(&device(1), &device(2), &key);
        assert_eq!(
            open(&key, &device(3).public(), &item, NOW, |_| true),
            Err(MailboxError::NotForUs)
        );
    }

    #[test]
    fn the_server_cant_redirect_or_extend_it() {
        let (phone, key) = (device(2), MailboxKey::from_seed([9; 32]));
        let item = sealed(&device(1), &phone, &key);

        let mut longer = item.clone();
        longer.header.expires_ms += 1;
        assert_eq!(
            open(&key, &phone.public(), &longer, NOW, |_| true),
            Err(MailboxError::Unreadable)
        );

        let mut renumbered = item.clone();
        renumbered.header.id = [8; 16];
        assert_eq!(
            open(&key, &phone.public(), &renumbered, NOW, |_| true),
            Err(MailboxError::Unreadable)
        );
    }

    #[test]
    fn it_cant_claim_another_sender() {
        // A device seals to the phone but names the laptop as sender: the
        // header is bound in, so the forged `from` fails to open; sealing
        // honestly under the laptop's name needs the laptop's key.
        let (laptop, phone, stranger) = (device(1), device(2), device(3));
        let key = MailboxKey::from_seed([9; 32]);
        let mut item = sealed(&stranger, &phone, &key);
        item.header.from = *laptop.public().as_bytes();
        assert_eq!(
            open(&key, &phone.public(), &item, NOW, |_| true),
            Err(MailboxError::Unreadable)
        );
    }

    #[test]
    fn a_signature_by_anyone_but_the_sender_is_refused() {
        // Sealed properly to the phone, under the laptop's name, but signed
        // inside by a stranger: the seal opens, the signature doesn't hold.
        let (laptop, phone, stranger) = (device(1), device(2), device(3));
        let key = MailboxKey::from_seed([9; 32]);
        let header = Header {
            version: VERSION,
            id: [7; 16],
            to: *phone.public().as_bytes(),
            from: *laptop.public().as_bytes(),
            expires_ms: LATER,
        };
        let item = seal_as(header, &stranger, &key.public(), b"hello").unwrap();
        assert_eq!(
            open(&key, &phone.public(), &item, NOW, |_| true),
            Err(MailboxError::BadSignature)
        );
    }

    #[test]
    fn it_is_refused_once_expired() {
        let (phone, key) = (device(2), MailboxKey::from_seed([9; 32]));
        let item = sealed(&device(1), &phone, &key);
        assert_eq!(
            open(&key, &phone.public(), &item, LATER, |_| true),
            Err(MailboxError::Expired)
        );
    }

    #[test]
    fn an_untrusted_sender_is_dropped() {
        let (laptop, phone) = (device(1), device(2));
        let key = MailboxKey::from_seed([9; 32]);
        let item = sealed(&laptop, &phone, &key);
        assert_eq!(
            open(&key, &phone.public(), &item, NOW, |_| false),
            Err(MailboxError::Untrusted)
        );
    }

    #[test]
    fn an_announcement_is_checked_against_its_device() {
        let (phone, key) = (device(2), MailboxKey::from_seed([9; 32]));
        let announcement = key.announce(&phone, NOW);
        assert_eq!(announcement.verify(&phone.public()), Ok(key.public()));
        assert_eq!(
            announcement.verify(&device(3).public()),
            Err(MailboxError::BadSignature)
        );
        let mut swapped = announcement.clone();
        swapped.key = MailboxKey::from_seed([8; 32]).public();
        assert_eq!(
            swapped.verify(&phone.public()),
            Err(MailboxError::BadSignature)
        );
    }

    #[test]
    fn a_new_mailbox_key_is_random_and_keeps_its_seed() {
        let key = MailboxKey::generate();
        assert_ne!(key.public(), MailboxKey::generate().public());
        assert_eq!(MailboxKey::from_seed(key.seed()).public(), key.public());
    }

    #[test]
    fn a_pass_is_registered_by_its_digest_only() {
        let pass = DepositPass::from_bytes([5; 32]);
        assert_ne!(pass.digest(), pass.bytes());
        assert_eq!(pass.digest(), DepositPass::from_bytes([5; 32]).digest());
        assert_ne!(DepositPass::generate().digest(), pass.digest());
    }

    /// A sealed item made by this version, kept as bytes: a later change
    /// that can't open it has changed the wire format and needs a new
    /// `VERSION`.
    const GOLDEN_ITEM: &str = include_str!("../tests/fixtures/mailbox-item-v1.hex");

    #[test]
    fn the_golden_item_still_opens() {
        let bytes = crate::hex::unhex(GOLDEN_ITEM.trim()).unwrap();
        let item: SealedItem = ciborium::from_reader(bytes.as_slice()).unwrap();
        let key = MailboxKey::from_seed([9; 32]);
        let opened = open(&key, &device(2).public(), &item, NOW, |from| {
            *from == device(1).public()
        })
        .unwrap();
        assert_eq!(opened.payload, b"golden");
        assert_eq!(opened.id, [7; 16]);
    }

    /// Writes the golden fixture: `cargo test --lib write_golden_item --
    /// --ignored`, only when `VERSION` changes.
    #[test]
    #[ignore]
    fn write_golden_item() {
        let key = MailboxKey::from_seed([9; 32]);
        let item = seal(
            &device(1),
            &device(2).public(),
            &key.public(),
            [7; 16],
            LATER,
            b"golden",
        )
        .unwrap();
        let mut bytes = Vec::new();
        ciborium::into_writer(&item, &mut bytes).unwrap();
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/mailbox-item-v1.hex"
            ),
            crate::hex::hex(&bytes) + "\n",
        )
        .unwrap();
    }

    /// N3 step 3: the mailbox key and passes come from the device key,
    /// the same each time, one pass per peer, so nothing new is stored.
    #[test]
    fn mailbox_secrets_are_derived_from_the_device_key() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let laptop = SecretKey::from_bytes(&[2; 32]).public();
        let tablet = SecretKey::from_bytes(&[3; 32]).public();
        assert_eq!(
            MailboxKey::of_device(&phone).public(),
            MailboxKey::of_device(&phone).public()
        );
        assert_ne!(
            MailboxKey::of_device(&phone).public(),
            MailboxKey::of_device(&SecretKey::from_bytes(&[4; 32])).public()
        );
        // Not the device key itself.
        assert_ne!(MailboxKey::of_device(&phone).seed(), phone.to_bytes());
        assert_eq!(
            DepositPass::for_peer(&phone, &laptop),
            DepositPass::for_peer(&phone, &laptop)
        );
        assert_ne!(
            DepositPass::for_peer(&phone, &laptop),
            DepositPass::for_peer(&phone, &tablet)
        );
    }

    /// An intro is kept only from the device that signed it; a newer one
    /// replaces the kept one only when it says something new.
    #[test]
    fn an_intro_is_kept_only_from_its_signer() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let laptop = SecretKey::from_bytes(&[2; 32]);
        let server = SecretKey::from_bytes(&[9; 32]).public();
        let intro = MailboxIntro::new(&phone, &laptop.public(), &server, 1_000);
        let kept = intro.verify(&phone.public()).unwrap();
        assert_eq!(kept.key(), Some(MailboxKey::of_device(&phone).public()));
        assert_eq!(
            kept.pass(),
            Some(DepositPass::for_peer(&phone, &laptop.public()))
        );
        assert_eq!(kept.server(), Some(server));
        assert_eq!(
            intro.verify(&laptop.public()),
            Err(MailboxError::BadSignature)
        );
        // The same intro later: nothing new, so nothing written.
        let later = MailboxIntro::new(&phone, &laptop.public(), &server, 2_000)
            .verify(&phone.public())
            .unwrap();
        assert!(!kept.replaced_by(&later));
        // Another server, later: replaces it; earlier: doesn't.
        let other = SecretKey::from_bytes(&[8; 32]).public();
        let moved = MailboxIntro::new(&phone, &laptop.public(), &other, 3_000)
            .verify(&phone.public())
            .unwrap();
        assert!(kept.replaced_by(&moved));
        assert!(!moved.replaced_by(&kept));
        // One that names a server that isn't a key is malformed.
        let not_a_key = (0..=255u8)
            .map(|b| [b; 32])
            .find(|b| PublicKey::from_bytes(b).is_err())
            .unwrap();
        let mut bad = intro.clone();
        bad.server = not_a_key;
        assert_eq!(bad.verify(&phone.public()), Err(MailboxError::Malformed));
    }
}
