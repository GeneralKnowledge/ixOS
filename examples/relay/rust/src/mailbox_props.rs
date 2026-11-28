//! V3 of docs/research/unsafe-and-verification.md: the mailbox server's
//! pass and quota rules against a plain in-memory model. Random sequences
//! of registering and deregistering passes, deposits (items and receipts,
//! by the right sender or not, under any pass, of any declared size and
//! life), acknowledgements, sweeps and the clock moving on run against the
//! real [`MailboxStore`] (SQLite and files in a temporary directory). After
//! every step the store's answer must be the model's, and:
//!
//! - an item is kept only under a pass its recipient registered;
//! - a pass registered by one recipient is never taken by another while
//!   it stays registered (once deregistered, its digest is free again);
//! - no pass or recipient ever holds more than its quota;
//! - a duplicate is never stored twice;
//! - what is listed is exactly what the model holds, unexpired.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use crate::mailbox::{DepositPass, Header, VERSION};
use crate::mailbox_server::*;

const START: i64 = 1_800_000_000_000;
const HOUR: i64 = 3600 * 1000;
const OWNERS: u8 = 3;
const PASSES: u8 = 6;
const IDS: u8 = 8;

fn owner(n: u8) -> [u8; 32] {
    [n + 1; 32]
}

fn pass(n: u8) -> [u8; 32] {
    [0x40 + n; 32]
}

fn digest(n: u8) -> [u8; 32] {
    DepositPass::from_bytes(pass(n)).digest()
}

#[derive(Clone, Debug)]
enum Op {
    Register(u8, Vec<u8>),
    Deregister(u8, Vec<u8>),
    Deposit {
        by: u8,
        from: u8,
        to: u8,
        pass: u8,
        id: u8,
        receipt: bool,
        len: u64,
        /// From now: negative is already expired.
        life: i64,
    },
    Ack(u8, u8, u8),
    Sweep,
    Wait(i64),
}

fn op() -> impl Strategy<Value = Op> {
    let who = 0..OWNERS;
    let len = prop_oneof![
        4 => 0..5000u64,
        2 => Just(60 * 1024 * 1024),
        5 => Just(MAX_ITEM),
        1 => Just(MAX_ITEM + 1),
        1 => Just(MAX_RECEIPT + 1),
    ];
    let life = prop_oneof![
        1 => -HOUR..=0,
        6 => 1..KEPT_MS,
        1 => Just(KEPT_MS + HOUR),
        1 => Just(KEPT_MS + 2 * HOUR),
    ];
    prop_oneof![
        3 => (who.clone(), proptest::collection::vec(0..PASSES, 0..3))
            .prop_map(|(o, ps)| Op::Register(o, ps)),
        1 => (who.clone(), proptest::collection::vec(0..PASSES, 0..3))
            .prop_map(|(o, ps)| Op::Deregister(o, ps)),
        8 => (
            (who.clone(), who.clone(), who.clone()),
            (0..PASSES, 0..IDS, any::<bool>()),
            len,
            life,
        )
            .prop_map(|((by, from, to), (pass, id, receipt), len, life)| Op::Deposit {
                by,
                // Mostly honest: the sender deposits as itself.
                from: if by % 2 == 0 { by } else { from },
                to,
                pass,
                id,
                receipt,
                len,
                life,
            }),
        2 => (who.clone(), who.clone(), 0..IDS).prop_map(|(o, f, i)| Op::Ack(o, f, i)),
        1 => Just(Op::Sweep),
        2 => prop_oneof![Just(60_000), Just(HOUR), Just(KEPT_MS / 2)].prop_map(Op::Wait),
    ]
}

#[derive(Clone, Debug)]
struct Kept {
    receipt: bool,
    pass: [u8; 32],
    size: u64,
    expires: i64,
}

/// An item's place: (recipient, sender, id).
type Slot = ([u8; 32], [u8; 32], [u8; 16]);

#[derive(Default, Debug)]
struct Model {
    passes: BTreeMap<[u8; 32], [u8; 32]>,
    items: BTreeMap<Slot, Kept>,
    deposits: Vec<([u8; 32], i64)>,
}

impl Model {
    fn held_by_pass(&self, pass: &[u8; 32]) -> u64 {
        self.items
            .values()
            .filter(|k| !k.receipt && &k.pass == pass)
            .map(|k| k.size)
            .sum()
    }

    fn held_by_owner(&self, owner: &[u8; 32]) -> u64 {
        self.items
            .iter()
            .filter(|((o, _, _), k)| o == owner && !k.receipt)
            .map(|(_, k)| k.size)
            .sum()
    }

    fn register(&mut self, o: [u8; 32], ds: &[[u8; 32]]) -> Result<(), Refusal> {
        if ds
            .iter()
            .any(|d| self.passes.get(d).is_some_and(|x| *x != o))
        {
            return Err(Refusal::NotYours);
        }
        for d in ds {
            self.passes.insert(*d, o);
        }
        Ok(())
    }

    /// What a deposit should do: `Ok(true)` stored, `Ok(false)` duplicate.
    #[allow(clippy::too_many_arguments)]
    fn deposit(
        &mut self,
        by: [u8; 32],
        header: &Header,
        pass: [u8; 32],
        receipt: bool,
        len: u64,
        now: i64,
    ) -> Result<bool, Refusal> {
        if header.from != by {
            return Err(Refusal::NotYours);
        }
        if header.expires_ms <= now {
            return Err(Refusal::Expired);
        }
        if header.expires_ms > now + KEPT_MS + HOUR {
            return Err(Refusal::TooLong);
        }
        if len > if receipt { MAX_RECEIPT } else { MAX_ITEM } {
            return Err(Refusal::TooBig);
        }
        if self.passes.get(&pass) != Some(&header.to) {
            return Err(Refusal::NoPass);
        }
        let key = (header.to, header.from, header.id);
        if self.items.contains_key(&key) {
            return Ok(false);
        }
        if !receipt {
            let hourly = self
                .deposits
                .iter()
                .filter(|(p, at)| *p == pass && *at > now - HOUR)
                .count();
            if hourly >= PER_PASS_HOURLY as usize {
                return Err(Refusal::TooMany);
            }
            if self.held_by_pass(&pass) + len > PER_PASS
                || self.held_by_owner(&header.to) + len > PER_RECIPIENT
            {
                return Err(Refusal::Full);
            }
            self.deposits.push((pass, now));
        }
        self.items.insert(
            key,
            Kept {
                receipt,
                pass,
                size: len,
                expires: header.expires_ms,
            },
        );
        Ok(true)
    }
}

/// The store's answer to a deposit: admitted, then kept, as the server
/// does once the bytes have arrived (the declared length is what counts).
fn deposit(
    store: &MailboxStore,
    dir: &std::path::Path,
    by: [u8; 32],
    deposit: Deposit,
    now: i64,
) -> Result<bool, Refusal> {
    let Some(admitted) = store.admit(&by, deposit, now)? else {
        return Ok(false);
    };
    let partial = dir.join("partial").join("upload");
    std::fs::write(&partial, b"bytes").unwrap();
    store.keep(admitted, &partial, || now)
}

fn check(store: &MailboxStore, model: &Model, now: i64) -> Result<(), TestCaseError> {
    for o in 0..OWNERS {
        let o = owner(o);
        let mine = model.passes.values().filter(|x| **x == o).count();
        prop_assert_eq!(store.passes_of(&o), mine);
        let waiting = model.items.keys().filter(|(x, _, _)| *x == o).count();
        prop_assert_eq!(store.waiting_for(&o), waiting);
        let listed: BTreeSet<_> = store
            .list(&o, now)
            .unwrap()
            .into_iter()
            .map(|w| (w.from, w.id, w.kind == Kind::Receipt, w.size))
            .collect();
        let expected: BTreeSet<_> = model
            .items
            .iter()
            .filter(|((x, _, _), k)| *x == o && k.expires > now)
            .map(|((_, f, i), k)| (*f, *i, k.receipt, k.size))
            .collect();
        prop_assert_eq!(listed, expected);
        prop_assert!(model.held_by_owner(&o) <= PER_RECIPIENT);
    }
    for p in 0..PASSES {
        prop_assert!(model.held_by_pass(&digest(p)) <= PER_PASS);
    }
    Ok(())
}

fn run(ops: &[Op]) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().unwrap();
    let store = MailboxStore::open(dir.path()).unwrap();
    let mut model = Model::default();
    let mut now = START;
    for op in ops {
        match op {
            Op::Register(o, ps) => {
                let ds: Vec<_> = ps.iter().map(|&p| digest(p)).collect();
                let got = store.register(&owner(*o), &ds);
                prop_assert_eq!(got, model.register(owner(*o), &ds), "{:?}", op);
            }
            Op::Deregister(o, ps) => {
                let ds: Vec<_> = ps.iter().map(|&p| digest(p)).collect();
                prop_assert_eq!(store.deregister(&owner(*o), &ds), Ok(()));
                for d in ds {
                    if model.passes.get(&d) == Some(&owner(*o)) {
                        model.passes.remove(&d);
                    }
                }
            }
            &Op::Deposit {
                by,
                from,
                to,
                pass: p,
                id,
                receipt,
                len,
                life,
            } => {
                let header = Header {
                    version: VERSION,
                    id: [id; 16],
                    to: owner(to),
                    from: owner(from),
                    expires_ms: now + life,
                };
                let expected = model.deposit(owner(by), &header, digest(p), receipt, len, now);
                let got = deposit(
                    &store,
                    dir.path(),
                    owner(by),
                    Deposit {
                        pass: pass(p),
                        kind: if receipt { Kind::Receipt } else { Kind::Item },
                        header,
                        enc: vec![0; 32],
                        len,
                    },
                    now,
                );
                prop_assert_eq!(got, expected, "{:?}", op);
            }
            &Op::Ack(o, f, i) => {
                prop_assert_eq!(store.ack(&owner(o), &owner(f), &[i; 16]), Ok(()));
                model.items.remove(&(owner(o), owner(f), [i; 16]));
            }
            Op::Sweep => {
                let gone = store.sweep(now);
                let before = model.items.len();
                model.items.retain(|_, k| k.expires > now);
                prop_assert_eq!(gone, before - model.items.len());
                model.deposits.retain(|(_, at)| *at > now - HOUR);
            }
            Op::Wait(ms) => now += ms,
        }
        check(&store, &model, now)?;
    }
    Ok(())
}

/// Deposits to one recipient that holds every pass: reaches the per-pass
/// and per-recipient quotas, which mixed sequences rarely fill.
fn filling() -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(
        (
            0..OWNERS,
            0..PASSES,
            0..IDS,
            prop_oneof![3 => Just(MAX_ITEM), 1 => 0..5000u64],
        ),
        1..40,
    )
    .prop_map(|deposits| {
        let mut ops = vec![Op::Register(0, (0..PASSES).collect())];
        ops.extend(
            deposits
                .into_iter()
                .map(|(from, pass, id, len)| Op::Deposit {
                    by: from,
                    from,
                    to: 0,
                    pass,
                    id,
                    receipt: false,
                    len,
                    life: HOUR,
                }),
        );
        ops
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn every_sequence_keeps_the_mailbox_rules(ops in proptest::collection::vec(op(), 1..80)) {
        run(&ops)?;
    }

    #[test]
    fn filling_a_mailbox_stops_at_its_quotas(ops in filling()) {
        run(&ops)?;
    }
}
