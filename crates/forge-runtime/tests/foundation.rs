//! Foundation review tests for forge-runtime (docs/foundation-review.md).
//!
//! Model-based property tests: proptest generates a random sequence of
//! operations, runs it against the real type and against a small reference
//! model written from the documented contract, and checks they agree after
//! every step. A failure shrinks to the shortest sequence that breaks the
//! contract.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use forge_runtime::net::Backoff;
use forge_runtime::{CancelHandle, Delivery, EventHub, Lifecycle, Phase, Subscription};
use proptest::prelude::*;

/// Polls `future` once, without blocking.
fn poll_once<F: Future>(future: F) -> Poll<F::Output> {
    let mut cx = Context::from_waker(Waker::noop());
    pin!(future).poll(&mut cx)
}

// ------------------------------------------------------------ event streams

#[derive(Debug, Clone)]
enum HubOp {
    Subscribe,
    Publish(u8),
    Next(usize),
    Cancel(usize),
    Drop(usize),
    Close,
}

fn hub_op() -> impl Strategy<Value = HubOp> {
    prop_oneof![
        2 => Just(HubOp::Subscribe),
        6 => any::<u8>().prop_map(HubOp::Publish),
        6 => any::<usize>().prop_map(HubOp::Next),
        1 => any::<usize>().prop_map(HubOp::Cancel),
        1 => any::<usize>().prop_map(HubOp::Drop),
        1 => Just(HubOp::Close),
    ]
}

/// One subscription as the contract describes it.
#[derive(Default)]
struct QueueModel {
    events: VecDeque<u8>,
    dropped: u64,
    finished: bool,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Event stream invariant: a bounded queue never silently breaks its
    /// delivery contract. Every event published while a subscription is
    /// live is either delivered, in order, or counted in the `Dropped`
    /// report that comes before the next delivery; a cancelled
    /// subscription delivers nothing more; a closed hub delivers what was
    /// queued, then ends.
    #[test]
    fn event_hub_matches_its_model(
        capacity in 1usize..5,
        ops in prop::collection::vec(hub_op(), 1..80),
    ) {
        let hub = EventHub::<u8>::with_capacity(capacity);
        let mut closed = false;
        let mut real: Vec<Subscription<u8>> = Vec::new();
        let mut model: Vec<QueueModel> = Vec::new();

        for op in ops {
            match op {
                HubOp::Subscribe => {
                    let sub = hub.subscribe();
                    prop_assert_eq!(sub.is_none(), closed);
                    if let Some(sub) = sub {
                        real.push(sub);
                        model.push(QueueModel::default());
                    }
                }
                HubOp::Publish(value) => {
                    hub.publish(value);
                    if !closed {
                        for queue in model.iter_mut().filter(|q| !q.finished) {
                            if queue.events.len() >= capacity {
                                queue.events.pop_front();
                                queue.dropped += 1;
                            }
                            queue.events.push_back(value);
                        }
                    }
                }
                HubOp::Next(i) if !real.is_empty() => {
                    let i = i % real.len();
                    let queue = &mut model[i];
                    let expected = if queue.dropped > 0 {
                        Poll::Ready(Some(Delivery::Dropped(std::mem::take(&mut queue.dropped))))
                    } else if let Some(event) = queue.events.pop_front() {
                        Poll::Ready(Some(Delivery::Event(event)))
                    } else if queue.finished {
                        Poll::Ready(None)
                    } else {
                        Poll::Pending
                    };
                    prop_assert_eq!(poll_once(real[i].next()), expected);
                }
                HubOp::Cancel(i) if !real.is_empty() => {
                    let i = i % real.len();
                    real[i].cancel();
                    model[i] = QueueModel { finished: true, ..QueueModel::default() };
                }
                HubOp::Drop(i) if !real.is_empty() => {
                    let i = i % real.len();
                    drop(real.remove(i));
                    model.remove(i);
                }
                HubOp::Close => {
                    hub.close();
                    closed = true;
                    for queue in &mut model {
                        queue.finished = true;
                    }
                }
                _ => {}
            }
            let live = model.iter().filter(|q| !q.finished).count();
            prop_assert_eq!(hub.subscriber_count(), live);
        }
    }
}

// ---------------------------------------------------------------- lifecycle

fn phase() -> impl Strategy<Value = Phase> {
    prop_oneof![
        Just(Phase::Launching),
        Just(Phase::Active),
        Just(Phase::Inactive),
        Just(Phase::Background),
        Just(Phase::Terminated),
    ]
}

#[derive(Debug, Clone)]
enum LifeOp {
    Enter(Phase),
    TakeForeground,
}

fn life_op() -> impl Strategy<Value = LifeOp> {
    prop_oneof![
        // Terminated ends everything, so keep it rarer than the rest.
        8 => phase().prop_filter("rare terminate", |p| *p != Phase::Terminated).prop_map(LifeOp::Enter),
        1 => Just(LifeOp::Enter(Phase::Terminated)),
        4 => Just(LifeOp::TakeForeground),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Lifecycle invariant: the phase machine and its cancellation scopes
    /// agree with the documented table. Work taken from a foreground scope
    /// is cancelled exactly when the app has entered the background (or
    /// terminated) since that scope began; `resumed` is set exactly on the
    /// way back from the background; `Terminated` is final; and every
    /// transition reaches a subscriber, in order.
    #[test]
    fn lifecycle_matches_its_model(ops in prop::collection::vec(life_op(), 1..50)) {
        let lifecycle = Lifecycle::new();
        let changes = lifecycle.subscribe().unwrap();
        let mut current = Phase::Launching;
        let mut scope = 0u32;
        let mut cancelled_scopes: HashSet<u32> = HashSet::new();
        let mut taken: Vec<(CancelHandle, u32)> = Vec::new();
        let mut transitions = Vec::new();

        for op in ops {
            match op {
                LifeOp::TakeForeground => taken.push((lifecycle.foreground(), scope)),
                LifeOp::Enter(to) => {
                    let got = lifecycle.enter(to);
                    if to == current || current == Phase::Terminated {
                        prop_assert_eq!(got, None);
                        continue;
                    }
                    let resumed = current == Phase::Background && to.is_foreground();
                    let transition = got.expect("a real change reports a transition");
                    prop_assert_eq!((transition.from, transition.to, transition.resumed), (current, to, resumed));
                    transitions.push(transition);
                    if to == Phase::Background {
                        cancelled_scopes.insert(scope);
                    } else if resumed {
                        scope += 1;
                    }
                    if to == Phase::Terminated {
                        cancelled_scopes.extend(0..=scope);
                    }
                    current = to;
                }
            }
            prop_assert_eq!(lifecycle.phase(), current);
            prop_assert_eq!(lifecycle.lifetime().is_cancelled(), current == Phase::Terminated);
            prop_assert_eq!(
                lifecycle.foreground().is_cancelled(),
                cancelled_scopes.contains(&scope),
            );
            for (handle, from_scope) in &taken {
                prop_assert_eq!(handle.is_cancelled(), cancelled_scopes.contains(from_scope));
            }
        }

        let mut seen = Vec::new();
        while let Poll::Ready(Some(delivery)) = poll_once(changes.next()) {
            match delivery {
                Delivery::Event(t) => seen.push(t),
                Delivery::Dropped(n) => prop_assert!(false, "{n} transitions dropped"),
            }
        }
        prop_assert_eq!(seen, transitions);
    }
}

// ------------------------------------------------------------- cancellation

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Cancellation invariant: in any tree of scopes, cancelling some nodes
    /// cancels exactly those nodes and every descendant still linked to
    /// them, and runs each cancelled node's hook exactly once.
    #[test]
    fn cancel_trees_reach_exactly_the_linked_descendants(
        // (parent choice, keep the link) for nodes 1..n; node 0 is the root.
        shape in prop::collection::vec((any::<usize>(), prop::bool::weighted(0.85)), 0..24),
        cancels in prop::collection::vec(any::<usize>(), 1..4),
    ) {
        let n = shape.len() + 1;
        let mut nodes = vec![CancelHandle::new()];
        let mut parent = vec![None];
        let mut links = Vec::new();
        for (i, (pick, keep)) in shape.iter().enumerate() {
            let p = pick % (i + 1);
            let (child, link) = nodes[p].child();
            nodes.push(child);
            if *keep {
                parent.push(Some(p));
                links.push(link);
            } else {
                parent.push(None);
                drop(link);
            }
        }
        let runs: Vec<Arc<AtomicUsize>> = (0..n).map(|_| Arc::new(AtomicUsize::new(0))).collect();
        let _hooks: Vec<_> = nodes
            .iter()
            .zip(&runs)
            .map(|(node, count)| {
                let count = Arc::clone(count);
                node.on_cancel(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                })
            })
            .collect();

        let roots: HashSet<usize> = cancels.iter().map(|c| c % n).collect();
        for &r in &roots {
            nodes[r].cancel();
            nodes[r].cancel(); // idempotent
        }
        for i in 0..n {
            let mut at = Some(i);
            let mut expected = false;
            while let Some(node) = at {
                if roots.contains(&node) {
                    expected = true;
                    break;
                }
                at = parent[node];
            }
            prop_assert_eq!(nodes[i].is_cancelled(), expected, "node {}", i);
            prop_assert_eq!(runs[i].load(Ordering::SeqCst), usize::from(expected), "hook of node {}", i);
        }
    }
}

/// Cancellation invariant under contention: hooks registered on other
/// threads while `cancel` runs still run exactly once each, whichever side
/// of the cancel they land on.
#[test]
fn hooks_registered_during_cancel_run_exactly_once() {
    for _ in 0..50 {
        let handle = CancelHandle::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let registrars: Vec<_> = (0..4)
            .map(|_| {
                let (handle, runs) = (handle.clone(), Arc::clone(&runs));
                std::thread::spawn(move || {
                    (0..100)
                        .map(|_| {
                            let runs = Arc::clone(&runs);
                            handle.on_cancel(move || {
                                runs.fetch_add(1, Ordering::SeqCst);
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handle.cancel();
        let kept: Vec<_> = registrars.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(runs.load(Ordering::SeqCst), 400);
        drop(kept);
    }
}

// ------------------------------------------------------------------ backoff

proptest! {
    /// Retry invariant: delays double from the initial delay up to the cap
    /// and never exceed it; jitter only ever shortens a delay, by at most
    /// its fraction; `reset` starts over.
    #[test]
    fn backoff_delays_stay_within_their_bounds(
        initial_ms in 1u64..2_000,
        max_ms in 1u64..120_000,
        jitter in 0.0f64..=1.0,
        steps in 1usize..40,
    ) {
        let initial = Duration::from_millis(initial_ms);
        let cap = Duration::from_millis(max_ms).max(initial);
        let mut plain = Backoff::new(initial, Duration::from_millis(max_ms));
        let mut jittered = Backoff::new(initial, Duration::from_millis(max_ms)).with_jitter(jitter);
        let mut base = initial;
        for _ in 0..steps {
            prop_assert_eq!(plain.next_delay(), base);
            let delay = jittered.next_delay();
            prop_assert!(delay <= base);
            prop_assert!(delay.as_secs_f64() >= base.as_secs_f64() * (1.0 - jitter) - 1e-9);
            base = base.saturating_mul(2).min(cap);
        }
        prop_assert!(plain.peek() <= cap);
        plain.reset();
        prop_assert_eq!(plain.next_delay(), initial);
    }
}

/// Retry invariant: a retry loop waiting on a cancelled handle stops at
/// once, even when its next delay is an hour.
#[test]
fn backoff_wait_respects_cancellation() {
    let cancel = CancelHandle::new();
    let mut backoff = Backoff::new(Duration::from_secs(3600), Duration::from_secs(3600));
    let waiter = {
        let cancel = cancel.clone();
        std::thread::spawn(move || backoff.wait(&cancel))
    };
    std::thread::sleep(Duration::from_millis(20));
    let started = std::time::Instant::now();
    cancel.cancel();
    assert!(!waiter.join().unwrap(), "wait reports the cancel");
    assert!(started.elapsed() < Duration::from_secs(1));
}
