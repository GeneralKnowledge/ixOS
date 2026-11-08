//! Foundation review tests for forge-services (docs/foundation-review.md).
//!
//! Contracts of `Feed`, the fan-out the optional services deliver host
//! values through.

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

/// Finding R2 (fixed): "cancelled work cannot continue producing events".
/// `Feed::publish` calls listeners outside its lock, and a listener used to
/// run after its `Watch::cancel()` had returned. `cancel` now waits for a
/// delivery already under way, and nothing reaches the listener after it.
/// In this test the cancel therefore waits for the first listener to be
/// released, so it runs on its own thread.
#[test]
fn no_value_reaches_a_listener_after_its_watch_is_cancelled() {
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
    let canceller = {
        let cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            second.cancel();
            cancelled.store(true, Ordering::SeqCst);
        })
    };
    std::thread::sleep(Duration::from_millis(50));
    release.wait();
    publisher.join().unwrap();
    canceller.join().unwrap();

    let called_after_cancel = late_rx
        .recv_timeout(Duration::from_millis(200))
        .unwrap_or(false);
    assert!(
        !called_after_cancel,
        "the listener ran after Watch::cancel() returned"
    );
}

/// A listener may cancel its own watch from inside a delivery without
/// waiting for itself.
#[test]
fn a_listener_can_cancel_its_own_watch() {
    let feed = Feed::<u32>::new();
    let slot: Arc<std::sync::Mutex<Option<forge_services::Watch>>> = Arc::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let watch = {
        let (slot, calls) = (Arc::clone(&slot), Arc::clone(&calls));
        feed.watch(
            move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                if let Some(watch) = slot.lock().unwrap().take() {
                    watch.cancel();
                }
            },
            || Ok::<_, ()>(()),
            || {},
        )
        .unwrap()
    };
    *slot.lock().unwrap() = Some(watch);
    feed.publish(1);
    feed.publish(2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(feed.listener_count(), 0);
}
