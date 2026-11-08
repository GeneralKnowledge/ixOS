//! Foundation review tests for forge-services (docs/foundation-review.md).
//!
//! `gap_*` tests are ignored: each asserts the contract the review says the
//! foundation should have and fails today. Run them with
//! `cargo test -p forge-services --test foundation -- --ignored`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use forge_services::Feed;

/// Feed invariant that holds: the host subscription starts with the first
/// listener and stops with the last, however watches come and go.
#[test]
fn a_feed_starts_once_and_stops_with_its_last_listener() {
    let feed = Feed::<u32>::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));
    let watch = |feed: &Arc<Feed<u32>>| {
        let (starts, stops) = (Arc::clone(&starts), Arc::clone(&stops));
        feed.watch(
            |_| {},
            move || {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            },
            move || {
                stops.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap()
    };
    for _ in 0..20 {
        let watches: Vec<_> = (0..3).map(|_| watch(&feed)).collect();
        assert_eq!(
            starts.load(Ordering::SeqCst),
            stops.load(Ordering::SeqCst) + 1
        );
        drop(watches);
        assert_eq!(starts.load(Ordering::SeqCst), stops.load(Ordering::SeqCst));
    }
    assert_eq!(feed.listener_count(), 0);
}

/// GAP (finding R2): "cancelled work cannot continue producing events".
/// `Feed::publish` copies the listener list and calls each listener outside
/// the lock, so a listener can still be called after its `Watch::cancel()`
/// has returned. Code that frees state when it cancels a watch (a view
/// model going away, a recording that has stopped) can then receive a
/// sample it no longer expects.
#[test]
#[ignore = "known gap R2: a cancelled watch can still receive a value"]
fn gap_no_value_reaches_a_listener_after_its_watch_is_cancelled() {
    let feed = Feed::<u32>::new();
    let ok = || Ok::<_, ()>(());

    // The first listener holds the publishing thread inside `publish`.
    let inside = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let _first = {
        let (inside, release) = (Arc::clone(&inside), Arc::clone(&release));
        feed.watch(
            move |_| {
                inside.wait();
                release.wait();
            },
            ok,
            || {},
        )
        .unwrap()
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let (late_tx, late_rx) = mpsc::channel();
    let second = {
        let cancelled = Arc::clone(&cancelled);
        feed.watch(
            move |_| {
                let _ = late_tx.send(cancelled.load(Ordering::SeqCst));
            },
            ok,
            || {},
        )
        .unwrap()
    };

    let publisher = {
        let feed = Arc::clone(&feed);
        std::thread::spawn(move || feed.publish(1))
    };
    inside.wait();
    second.cancel();
    cancelled.store(true, Ordering::SeqCst);
    release.wait();
    publisher.join().unwrap();

    let called_after_cancel = late_rx
        .recv_timeout(Duration::from_millis(200))
        .unwrap_or(false);
    assert!(
        !called_after_cancel,
        "the listener ran after Watch::cancel() returned"
    );
}
