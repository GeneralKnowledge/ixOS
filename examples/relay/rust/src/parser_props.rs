//! V4 of docs/research/unsafe-and-verification.md: everything Relay reads
//! from the network, fed random and damaged bytes. None of it may panic,
//! and nothing damaged may be accepted beyond what is not signed:
//!
//! - a mailbox server request of any bytes is refused or answered, and
//!   whatever decodes is run against a real store;
//! - a `relay/device/1` message of any bytes decodes or is refused;
//! - a sealed item with any byte changed, or made of random parts, never
//!   opens; a mailbox intro with any byte changed never yields a different
//!   key or issue date (its pass and server aren't signed: the
//!   authenticated Hello carries them).
//!
//! Longer runs: `PROPTEST_CASES=100000 cargo test --lib parser_props`.

use iroh::SecretKey;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

use crate::device::DeviceMsg;
use crate::mailbox::{open, seal, MailboxIntro, MailboxKey, SealedItem};
use crate::mailbox_server::{MailboxStore, Request};

const NOW: i64 = 1_800_000_000_000;

/// `n` cases, or `PROPTEST_CASES` for a longer run.
fn cases(n: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes(&[n; 32])
}

fn cbor<T: serde::Serialize>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).unwrap();
    out
}

/// `bytes` with each `(position, xor)` applied; a zero xor still changes
/// nothing, so callers skip unchanged copies.
fn damaged(bytes: &[u8], flips: &[(usize, u8)]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for &(at, x) in flips {
        let at = at % out.len();
        out[at] ^= x;
    }
    out
}

fn flips() -> impl Strategy<Value = Vec<(usize, u8)>> {
    proptest::collection::vec((any::<usize>(), 1..=255u8), 1..4)
}

/// Bytes worth decoding: random, or a CBOR value with random contents.
fn wire() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..300),
        // Small maps and arrays CBOR decoders walk into.
        proptest::collection::vec(
            prop_oneof![Just(0xa1u8), Just(0x82), Just(0x58), any::<u8>()],
            0..64
        ),
    ]
}

#[test]
fn any_request_to_the_mailbox_server_is_refused_or_answered() {
    let dir = tempfile::tempdir().unwrap();
    let store = MailboxStore::open(dir.path()).unwrap();
    let caller = [1u8; 32];
    let mut runner = TestRunner::new(Config {
        cases: cases(3000),
        source_file: Some(file!()),
        ..Config::default()
    });
    runner
        .run(&wire(), |bytes| {
            let Ok(request) = ciborium::from_reader::<Request, _>(bytes.as_slice()) else {
                return Ok(());
            };
            // What the server does with it, short of the network.
            match request {
                Request::Register { digests } => drop(store.register(&caller, &digests)),
                Request::Deregister { digests } => drop(store.deregister(&caller, &digests)),
                Request::List => drop(store.list(&caller, NOW)),
                Request::Fetch { from, id } => drop(store.fetch(&caller, &from, &id, NOW)),
                Request::Ack { from, id } => drop(store.ack(&caller, &from, &id)),
                Request::Deposit(deposit) => drop(store.admit(&caller, deposit, NOW)),
            }
            Ok(())
        })
        .unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(2000)))]

    #[test]
    fn any_device_message_decodes_or_is_refused(bytes in wire()) {
        if let Ok(msg) = DeviceMsg::decode(&bytes) {
            // Whatever decodes encodes again without trouble.
            prop_assert!(DeviceMsg::decode(&msg.encode()).is_ok());
        }
    }

    #[test]
    fn a_damaged_sealed_item_never_opens(f in flips()) {
        let (laptop, phone) = (key(1), key(2));
        let mailbox = MailboxKey::from_seed([9; 32]);
        let item = seal(&laptop, &phone.public(), &mailbox.public(), [7; 16], NOW + 1000, b"hello").unwrap();
        let bytes = cbor(&item);
        let changed = damaged(&bytes, &f);
        prop_assume!(changed != bytes);
        if let Ok(other) = ciborium::from_reader::<SealedItem, _>(changed.as_slice()) {
            // CBOR can spell the same item two ways; only a different
            // item must be refused.
            if cbor(&other) != bytes {
                let opened = open(&mailbox, &phone.public(), &other, NOW, |_| true);
                prop_assert!(opened.is_err(), "a damaged item opened: {:?}", opened);
            }
        }
    }

    #[test]
    fn a_sealed_item_of_random_parts_never_opens(
        id in any::<[u8; 16]>(),
        enc in proptest::collection::vec(any::<u8>(), 0..64),
        ciphertext in proptest::collection::vec(any::<u8>(), 0..200),
        expires in NOW + 1..NOW + 1_000_000,
    ) {
        let phone = key(2);
        let mailbox = MailboxKey::from_seed([9; 32]);
        let item = SealedItem {
            header: crate::mailbox::Header {
                version: crate::mailbox::VERSION,
                id,
                to: *phone.public().as_bytes(),
                from: *key(1).public().as_bytes(),
                expires_ms: expires,
            },
            enc,
            ciphertext,
        };
        prop_assert!(open(&mailbox, &phone.public(), &item, NOW, |_| true).is_err());
    }

    #[test]
    fn a_damaged_mailbox_intro_never_yields_another_key(f in flips()) {
        let laptop = key(2);
        let intro = MailboxIntro::new(&laptop, &key(1).public(), &key(9).public(), NOW);
        let bytes = cbor(&intro);
        let changed = damaged(&bytes, &f);
        if let Ok(other) = ciborium::from_reader::<MailboxIntro, _>(changed.as_slice()) {
            let good = intro.verify(&laptop.public()).unwrap();
            if let Ok(got) = other.verify(&laptop.public()) {
                // Only the pass and the server aren't signed: the key and
                // its date must be exactly what the device announced.
                prop_assert_eq!(got.key, good.key);
                prop_assert_eq!(got.issued_ms, good.issued_ms);
            }
        }
    }
}
