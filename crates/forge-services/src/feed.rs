//! Live values from the host (connectivity, battery, sensors, location),
//! shared by every Rust listener in the app.
//!
//! The host runs one platform subscription per feed, however many listeners
//! Rust has: the first [`Feed::watch`] starts it and dropping the last
//! [`Watch`] stops it. Each value is passed to every listener, on the thread
//! the host delivered it on, so listeners should be quick (copy the value
//! into your own state and return).

use std::sync::{Arc, Mutex, MutexGuard, Weak};

type Listener<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// One stream of values from the host, fanned out to Rust listeners.
pub struct Feed<T> {
    state: Mutex<State<T>>,
    /// Serialises "first listener starts the host" against "last listener
    /// stops it", so a stop can never overtake a later start.
    control: Mutex<()>,
}

struct State<T> {
    next_id: u64,
    listeners: Vec<(u64, Listener<T>)>,
    latest: Option<T>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl<T: Clone + Send + 'static> Feed<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                next_id: 0,
                listeners: Vec::new(),
                latest: None,
            }),
            control: Mutex::new(()),
        })
    }

    /// Passes `value` to every listener and keeps it as [`Feed::latest`].
    /// Called by the host (through the module's sink object).
    pub fn publish(&self, value: T) {
        let listeners: Vec<Listener<T>> = {
            let mut state = lock(&self.state);
            state.latest = Some(value.clone());
            state.listeners.iter().map(|(_, l)| l.clone()).collect()
        };
        for listener in listeners {
            listener(&value);
        }
    }

    /// The last value published, kept after the last listener leaves (a
    /// last-known value is still useful).
    pub fn latest(&self) -> Option<T> {
        lock(&self.state).latest.clone()
    }

    pub fn listener_count(&self) -> usize {
        lock(&self.state).listeners.len()
    }

    /// Adds `listener`. When it is the first, `start` runs (it asks the host
    /// to begin delivering); if `start` fails the listener is removed and
    /// the error returned. Dropping or cancelling the returned [`Watch`]
    /// removes the listener, and `stop` runs when it was the last one.
    pub fn watch<E>(
        self: &Arc<Self>,
        listener: impl Fn(&T) + Send + Sync + 'static,
        start: impl FnOnce() -> Result<(), E>,
        stop: impl FnOnce() + Send + 'static,
    ) -> Result<Watch, E> {
        let _control = lock(&self.control);
        let (id, first) = {
            let mut state = lock(&self.state);
            let id = state.next_id;
            state.next_id += 1;
            state.listeners.push((id, Arc::new(listener)));
            (id, state.listeners.len() == 1)
        };
        if first {
            if let Err(err) = start() {
                lock(&self.state)
                    .listeners
                    .retain(|(other, _)| *other != id);
                return Err(err);
            }
        }
        let feed: Weak<Self> = Arc::downgrade(self);
        Ok(Watch::new(move || {
            if let Some(feed) = feed.upgrade() {
                let _control = lock(&feed.control);
                let now_empty = {
                    let mut state = lock(&feed.state);
                    let before = state.listeners.len();
                    state.listeners.retain(|(other, _)| *other != id);
                    before != state.listeners.len() && state.listeners.is_empty()
                };
                if now_empty {
                    stop();
                }
            }
        }))
    }
}

/// A listener on a [`Feed`]. Dropping it (or calling [`Watch::cancel`])
/// stops the deliveries to this listener.
pub struct Watch {
    cancel: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Watch {
    pub fn new(cancel: impl FnOnce() + Send + 'static) -> Self {
        Self {
            cancel: Mutex::new(Some(Box::new(cancel))),
        }
    }

    /// Stops this listener now. Calling it again does nothing.
    pub fn cancel(&self) {
        let cancel = lock(&self.cancel).take();
        if let Some(cancel) = cancel {
            cancel();
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counter() -> (Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let c = Arc::new(AtomicUsize::new(0));
        (c.clone(), c)
    }

    #[test]
    fn first_listener_starts_and_last_stops() {
        let feed = Feed::<u32>::new();
        let (starts, s) = counter();
        let (stops, t) = counter();
        let start = || {
            s.fetch_add(1, Ordering::SeqCst);
            Ok::<(), ()>(())
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_a = seen.clone();
        let t2 = t.clone();
        let a = feed
            .watch(
                move |v| seen_a.lock().unwrap().push(*v),
                start,
                move || {
                    t2.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        let s2 = starts.clone();
        let b = feed
            .watch(
                |_| {},
                move || {
                    s2.fetch_add(1, Ordering::SeqCst);
                    Ok::<(), ()>(())
                },
                move || {
                    t.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        feed.publish(7);
        assert_eq!(*seen.lock().unwrap(), vec![7]);
        assert_eq!(feed.latest(), Some(7));

        drop(a);
        assert_eq!(stops.load(Ordering::SeqCst), 0);
        feed.publish(8);
        assert_eq!(*seen.lock().unwrap(), vec![7], "a no longer listens");
        b.cancel();
        b.cancel();
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        assert_eq!(feed.listener_count(), 0);
        assert_eq!(feed.latest(), Some(8), "last value is kept");
        drop(b);
        assert_eq!(
            stops.load(Ordering::SeqCst),
            1,
            "cancel then drop stops once"
        );
    }

    #[test]
    fn failed_start_removes_the_listener() {
        let feed = Feed::<u32>::new();
        let err = feed.watch(
            |_| {},
            || Err("no sensor"),
            || panic!("not started, so not stopped"),
        );
        assert_eq!(err.unwrap_err(), "no sensor");
        assert_eq!(feed.listener_count(), 0);
        // The next listener is the first again and starts the host.
        let (starts, s) = counter();
        let _w = feed
            .watch(
                |_| {},
                move || {
                    s.fetch_add(1, Ordering::SeqCst);
                    Ok::<(), ()>(())
                },
                || {},
            )
            .unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn listener_may_publish_reentrantly_free_of_deadlock() {
        // A listener that reads the feed while a value is being delivered.
        let feed = Feed::<u32>::new();
        let inner = feed.clone();
        let _w = feed
            .watch(
                move |_| {
                    let _ = inner.latest();
                    let _ = inner.listener_count();
                },
                || Ok::<(), ()>(()),
                || {},
            )
            .unwrap();
        feed.publish(1);
    }

    #[test]
    fn watch_outliving_its_feed_is_harmless() {
        let feed = Feed::<u32>::new();
        let w = feed.watch(|_| {}, || Ok::<(), ()>(()), || {}).unwrap();
        drop(feed);
        drop(w);
    }
}
