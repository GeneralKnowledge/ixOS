//! V2 of docs/research/unsafe-and-verification.md: the trust rules as a
//! state machine. Random sequences of everything that can change a
//! device's standing or rights (the person's choices, code transfers under
//! each policy, introducers' lists, Hellos) run against the real
//! [`Devices`], and the rules are checked after every step:
//!
//! - Only the person trusts a revoked device again (`trust`,
//!   `trust_introduced`); no code, policy or list does.
//! - A device trusted again starts with no rights beyond receiving.
//! - Anything granted, any mailbox kept, any right to introduce or see the
//!   list belongs to a trusted device.
//! - A person's device never introduces or sees the list.
//! - An introducer's list never trusts anyone, never changes the
//!   introducer or this device, and counts only while the introducer is
//!   trusted and introduces; every device it revokes that is known here
//!   ends revoked.
//! - After a code transfer a device becomes trusted only if it was
//!   forgotten and the policy is Pair Automatically.

use proptest::prelude::*;

use crate::capability::Capability;
use crate::mailbox::PeerMailbox;
use crate::trust::*;

const ME: &str = "me";
const IDS: [&str; 5] = ["a", "b", "c", "d", ME];

#[derive(Clone, Debug)]
enum Op {
    Trust(usize),
    TrustIntroduced(usize, usize),
    Revoke(usize),
    Forget(usize),
    AfterCode(usize),
    Policy(PairingPolicy),
    Introduces(usize, bool),
    SeesDevices(usize, bool),
    Person(usize, bool),
    Grant(usize, bool),
    ReceivesFrom(usize, bool),
    MayAsk(usize, bool),
    Met(usize),
    Introduced {
        introducer: usize,
        trusted: Vec<usize>,
        revoked: Vec<usize>,
    },
}

fn id() -> impl Strategy<Value = usize> {
    0..IDS.len()
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => id().prop_map(Op::Trust),
        1 => (id(), id()).prop_map(|(d, by)| Op::TrustIntroduced(d, by)),
        2 => id().prop_map(Op::Revoke),
        2 => id().prop_map(Op::Forget),
        3 => id().prop_map(Op::AfterCode),
        1 => prop_oneof![
            Just(PairingPolicy::CodesOnly),
            Just(PairingPolicy::AskToPair),
            Just(PairingPolicy::PairAutomatically),
        ]
        .prop_map(Op::Policy),
        2 => (id(), any::<bool>()).prop_map(|(d, on)| Op::Introduces(d, on)),
        1 => (id(), any::<bool>()).prop_map(|(d, on)| Op::SeesDevices(d, on)),
        1 => (id(), any::<bool>()).prop_map(|(d, on)| Op::Person(d, on)),
        2 => (id(), any::<bool>()).prop_map(|(d, on)| Op::Grant(d, on)),
        1 => (id(), any::<bool>()).prop_map(|(d, on)| Op::ReceivesFrom(d, on)),
        1 => (id(), any::<bool>()).prop_map(|(d, on)| Op::MayAsk(d, on)),
        1 => id().prop_map(Op::Met),
        3 => (
            id(),
            proptest::collection::vec(id(), 0..4),
            proptest::collection::vec(id(), 0..4)
        )
            .prop_map(|(introducer, trusted, revoked)| Op::Introduced {
                introducer,
                trusted,
                revoked,
            }),
    ]
}

fn mailbox(n: i64) -> PeerMailbox {
    PeerMailbox {
        key: format!("{n:064x}"),
        issued_ms: n,
        pass: format!("{n:064x}"),
        server: "server".into(),
    }
}

/// Applies `op` at time `now`; returns the introducer's result, if any.
fn apply(devices: &mut Devices, op: &Op, now: i64) -> Option<Introduced> {
    match *op {
        Op::Trust(d) => devices.trust(IDS[d], "Name", now),
        Op::TrustIntroduced(d, by) => devices.trust_introduced(IDS[d], "Name", IDS[by], now),
        Op::Revoke(d) => {
            devices.revoke(IDS[d]);
        }
        Op::Forget(d) => {
            devices.forget(IDS[d]);
        }
        Op::AfterCode(d) => {
            devices.after_code(IDS[d], "Name", now);
        }
        Op::Policy(p) => devices.policy = p,
        Op::Introduces(d, on) => {
            devices.set_introduces(IDS[d], on);
        }
        Op::SeesDevices(d, on) => {
            devices.set_sees_devices(IDS[d], on);
        }
        Op::Person(d, on) => {
            devices.set_person(IDS[d], on.then_some("Sam"));
        }
        Op::Grant(d, on) => {
            devices.set_grant(IDS[d], Capability::Sites, on);
        }
        Op::ReceivesFrom(d, on) => {
            devices.set_receives_from(IDS[d], on);
        }
        Op::MayAsk(d, on) => {
            devices.set_may_ask(IDS[d], on);
        }
        Op::Met(d) => devices.met(IDS[d], &[], None, Some(mailbox(now)), now),
        Op::Introduced {
            introducer,
            ref trusted,
            ref revoked,
        } => {
            let list = DeviceList {
                trusted: trusted
                    .iter()
                    .map(|&d| (IDS[d].to_string(), "Name".to_string()))
                    .collect(),
                revoked: revoked.iter().map(|&d| IDS[d].to_string()).collect(),
            };
            return Some(devices.introduced(IDS[introducer], ME, &list));
        }
    }
    None
}

/// What must hold in every state.
fn check_state(devices: &Devices) -> Result<(), TestCaseError> {
    let mut seen = std::collections::HashSet::new();
    for d in &devices.list {
        prop_assert!(
            seen.insert(&d.endpoint_id),
            "listed twice: {}",
            d.endpoint_id
        );
        let trusted = d.trust == SavedTrust::Trusted;
        prop_assert_eq!(trusted, may_connect(Some(d.trust)));
        if !trusted {
            prop_assert!(
                d.grants.is_empty(),
                "{} holds grants untrusted",
                d.endpoint_id
            );
            prop_assert!(d.capabilities().is_empty());
            prop_assert!(
                d.mailbox.is_none(),
                "{} kept a mailbox untrusted",
                d.endpoint_id
            );
        }
        if d.person.is_some() {
            prop_assert!(
                !d.introduces && !d.sees_devices,
                "{}: a person's device",
                d.endpoint_id
            );
        }
        for c in Capability::ALL {
            prop_assert_eq!(devices.granted(&d.endpoint_id, c), d.has(c));
            if d.has(c) {
                prop_assert!(trusted);
            }
        }
    }
    Ok(())
}

/// What must hold for one step from `before` to `after`.
fn check_step(
    before: &Devices,
    after: &Devices,
    op: &Op,
    result: Option<&Introduced>,
) -> Result<(), TestCaseError> {
    for &id in &IDS {
        let was = before.standing(id);
        let now = after.standing(id);
        let by_person = matches!(*op, Op::Trust(d) | Op::TrustIntroduced(d, _) if IDS[d] == id);
        if now == Some(SavedTrust::Trusted) && was != Some(SavedTrust::Trusted) {
            let by_policy = matches!(*op, Op::AfterCode(d) if IDS[d] == id)
                && was == Some(SavedTrust::Forgotten)
                && before.policy == PairingPolicy::PairAutomatically;
            prop_assert!(
                by_person || by_policy,
                "{id} became trusted ({was:?}) by {op:?}"
            );
        }
        if was == Some(SavedTrust::Revoked) && !by_person {
            prop_assert_eq!(
                now,
                Some(SavedTrust::Revoked),
                "{} left revoked by {:?}",
                id,
                op
            );
        }
        if by_person {
            let d = after.get(id).unwrap();
            prop_assert!(d.receives_from && !d.may_ask && !d.introduces && !d.sees_devices);
            prop_assert!(d.grants.is_empty() && d.mailbox.is_none() && d.revoked_by.is_none());
        }
    }
    if let Op::Introduced {
        introducer,
        ref revoked,
        ..
    } = *op
    {
        let introducer = IDS[introducer];
        let counts = before
            .get(introducer)
            .is_some_and(|d| d.trust == SavedTrust::Trusted && d.introduces);
        let result = result.unwrap();
        if !counts {
            prop_assert_eq!(
                before,
                after,
                "a list from a non-introducer changed something"
            );
            prop_assert_eq!(result, &Introduced::default());
        }
        prop_assert_eq!(before.get(introducer), after.get(introducer));
        prop_assert_eq!(before.get(ME), after.get(ME));
        for (offered, _) in &result.offers {
            prop_assert_ne!(after.standing(offered), Some(SavedTrust::Trusted));
            prop_assert_ne!(after.standing(offered), Some(SavedTrust::Revoked));
        }
        if counts {
            for &r in revoked {
                let r = IDS[r];
                if r != introducer && r != ME && before.get(r).is_some() {
                    prop_assert_eq!(
                        after.standing(r),
                        Some(SavedTrust::Revoked),
                        "{} not revoked",
                        r
                    );
                }
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn every_sequence_keeps_the_trust_rules(ops in proptest::collection::vec(op(), 1..60)) {
        let mut devices = Devices::default();
        for (i, op) in ops.iter().enumerate() {
            let before = devices.clone();
            let result = apply(&mut devices, op, i as i64 + 1);
            check_state(&devices)?;
            check_step(&before, &devices, op, result.as_ref())?;
        }
    }
}
