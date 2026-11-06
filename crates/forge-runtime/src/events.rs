//! Bounded, pull-based event streams for the FFI boundary.
//!
//! The app publishes events into an [`EventHub`]; each host consumer holds a
//! [`Subscription`] and awaits [`Subscription::next`]. Nothing calls into
//! Swift or Kotlin from a Rust thread, so a slow consumer cannot stall the
//! app. Each subscription buffers up to its capacity; beyond that the oldest
//! events are dropped and the consumer gets [`Delivery::Dropped`] first, so
//! it knows to re-read the current state.
//!
//! UniFFI objects cannot be generic, so an app wraps a `Subscription<E>` in
//! its own `#[derive(uniffi::Object)]` (see `docs/app-crate.md`).

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::task::{Context, Poll, Waker};

/// Events buffered per subscription before the oldest are dropped.
pub const DEFAULT_QUEUE_CAPACITY: usize = 64;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What [`Subscription::next`] yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery<E> {
    Event(E),
    /// This consumer fell behind and `count` older events were discarded.
    Dropped(u64),
}

/// Fan-out to the current subscribers. Cheap to clone; clones share state.
pub struct EventHub<E> {
    inner: Arc<Hub<E>>,
}

impl<E> Clone for EventHub<E> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Hub<E> {
    capacity: usize,
    state: Mutex<HubState<E>>,
}

struct HubState<E> {
    closed: bool,
    subscribers: Vec<Arc<Queue<E>>>,
}

impl<E: Clone + Send + 'static> Default for EventHub<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Clone + Send + 'static> EventHub<E> {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_QUEUE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Hub {
                capacity: capacity.max(1),
                state: Mutex::new(HubState {
                    closed: false,
                    subscribers: Vec::new(),
                }),
            }),
        }
    }

    /// A new subscription that sees every event published from now on, or
    /// `None` once the hub is closed.
    pub fn subscribe(&self) -> Option<Subscription<E>> {
        let queue = Arc::new(Queue::new(self.inner.capacity));
        let mut state = lock(&self.inner.state);
        if state.closed {
            return None;
        }
        state.subscribers.push(Arc::clone(&queue));
        Some(Subscription {
            queue,
            hub: Arc::downgrade(&self.inner),
        })
    }

    /// Queues `event` for every subscriber. Does nothing once closed.
    pub fn publish(&self, event: E) {
        let state = lock(&self.inner.state);
        if state.closed {
            return;
        }
        for subscriber in &state.subscribers {
            subscriber.push(event.clone());
        }
    }

    /// Ends every subscription (queued events are still delivered, then
    /// `next()` returns `None`) and refuses new ones. Idempotent.
    pub fn close(&self) {
        let subscribers = {
            let mut state = lock(&self.inner.state);
            state.closed = true;
            std::mem::take(&mut state.subscribers)
        };
        for subscriber in subscribers {
            subscriber.finish(false);
        }
    }

    pub fn subscriber_count(&self) -> usize {
        lock(&self.inner.state).subscribers.len()
    }
}

impl<E> Hub<E> {
    fn remove(&self, queue: &Arc<Queue<E>>) {
        lock(&self.state)
            .subscribers
            .retain(|s| !Arc::ptr_eq(s, queue));
    }
}

struct Queue<E> {
    capacity: usize,
    state: Mutex<QueueState<E>>,
}

struct QueueState<E> {
    events: VecDeque<E>,
    dropped: u64,
    finished: bool,
    waker: Option<Waker>,
}

impl<E> Queue<E> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(QueueState {
                events: VecDeque::new(),
                dropped: 0,
                finished: false,
                waker: None,
            }),
        }
    }

    fn push(&self, event: E) {
        let waker = {
            let mut state = lock(&self.state);
            if state.finished {
                return;
            }
            if state.events.len() >= self.capacity {
                state.events.pop_front();
                state.dropped += 1;
            }
            state.events.push_back(event);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn finish(&self, discard: bool) {
        let waker = {
            let mut state = lock(&self.state);
            state.finished = true;
            if discard {
                state.events.clear();
                state.dropped = 0;
            }
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn poll_next(&self, cx: &mut Context<'_>) -> Poll<Option<Delivery<E>>> {
        let mut state = lock(&self.state);
        if state.dropped > 0 {
            return Poll::Ready(Some(Delivery::Dropped(std::mem::take(&mut state.dropped))));
        }
        if let Some(event) = state.events.pop_front() {
            return Poll::Ready(Some(Delivery::Event(event)));
        }
        if state.finished {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// One consumer's stream. Use it from a single consumer at a time. It ends
/// on [`cancel`](Self::cancel), on drop, or when the hub closes.
pub struct Subscription<E> {
    queue: Arc<Queue<E>>,
    hub: Weak<Hub<E>>,
}

impl<E: Send + 'static> Subscription<E> {
    /// The next delivery, or `None` once the subscription has ended.
    pub fn next(&self) -> impl Future<Output = Option<Delivery<E>>> + Send + 'static {
        NextDelivery {
            queue: Arc::clone(&self.queue),
        }
    }

    /// Ends the subscription: a pending `next()` returns `None` at once and
    /// undelivered events are discarded. Idempotent.
    pub fn cancel(&self) {
        self.queue.finish(true);
        if let Some(hub) = self.hub.upgrade() {
            hub.remove(&self.queue);
        }
    }
}

impl<E> Drop for Subscription<E> {
    fn drop(&mut self) {
        self.queue.finish(true);
        if let Some(hub) = self.hub.upgrade() {
            hub.remove(&self.queue);
        }
    }
}

struct NextDelivery<E> {
    queue: Arc<Queue<E>>,
}

impl<E> Future for NextDelivery<E> {
    type Output = Option<Delivery<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.queue.poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{background, block_on};
    use std::time::Duration;

    #[test]
    fn every_subscriber_sees_events_in_order() {
        let hub = EventHub::new();
        let (a, b) = (hub.subscribe().unwrap(), hub.subscribe().unwrap());
        hub.publish(1);
        hub.publish(2);
        for sub in [&a, &b] {
            assert_eq!(block_on(sub.next()), Some(Delivery::Event(1)));
            assert_eq!(block_on(sub.next()), Some(Delivery::Event(2)));
        }
    }

    #[test]
    fn a_waiting_consumer_is_woken_by_publish() {
        let hub = EventHub::new();
        let sub = hub.subscribe().unwrap();
        let publisher = hub.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            publisher.publish("hello");
        });
        assert_eq!(block_on(sub.next()), Some(Delivery::Event("hello")));
        thread.join().unwrap();
    }

    #[test]
    fn overflow_drops_oldest_and_reports_the_count() {
        let hub = EventHub::with_capacity(2);
        let sub = hub.subscribe().unwrap();
        for i in 0..5 {
            hub.publish(i);
        }
        assert_eq!(block_on(sub.next()), Some(Delivery::Dropped(3)));
        assert_eq!(block_on(sub.next()), Some(Delivery::Event(3)));
        assert_eq!(block_on(sub.next()), Some(Delivery::Event(4)));
    }

    #[test]
    fn close_delivers_queued_events_then_ends() {
        let hub = EventHub::new();
        let sub = hub.subscribe().unwrap();
        hub.publish(1);
        hub.close();
        assert_eq!(block_on(sub.next()), Some(Delivery::Event(1)));
        assert_eq!(block_on(sub.next()), None);
        assert!(hub.subscribe().is_none());
    }

    #[test]
    fn cancel_ends_a_pending_next_and_unregisters() {
        let hub = EventHub::<u8>::new();
        let sub = Arc::new(hub.subscribe().unwrap());
        let waiting = {
            let next = sub.next();
            background("wait", move || block_on(next))
        };
        std::thread::sleep(Duration::from_millis(50));
        sub.cancel();
        assert_eq!(block_on(waiting), None);
        assert_eq!(hub.subscriber_count(), 0);
        drop(sub);
    }

    #[test]
    fn dropping_a_subscription_unregisters_it() {
        let hub = EventHub::<u8>::new();
        drop(hub.subscribe().unwrap());
        assert_eq!(hub.subscriber_count(), 0);
    }
}
