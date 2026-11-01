//! Rust → host events.
//!
//! Hosts *pull* events: [`ForgeCounterApp::subscribe`](crate::ForgeCounterApp::subscribe)
//! returns a [`CounterEventSubscription`] whose async `next()` resolves with the
//! next event, or `None` once the subscription has ended. Nothing calls into
//! Swift or Kotlin from a Rust thread, so host code never runs on the serial
//! command thread and a slow consumer cannot stall it.
//!
//! Each subscription has a bounded queue ([`QUEUE_CAPACITY`]). When it is full
//! the oldest event is discarded and the next `next()` returns
//! [`CounterEvent::EventsDropped`] first, telling the consumer to re-read the
//! current state with `snapshot()`.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use crate::api::CounterStateSnapshot;
use crate::errors::ForgeAppError;
use crate::signal::lock;

/// Events buffered per subscription before the oldest are dropped.
pub const QUEUE_CAPACITY: usize = 64;

/// Something meaningful that happened in the Rust app.
///
/// Events are emitted only after the change they describe has been persisted.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum CounterEvent {
    /// The committed counter value changed (from any command or operation).
    StateChanged { snapshot: CounterStateSnapshot },
    /// A long-running operation made progress. `completed` ≤ `total`.
    OperationProgress {
        operation_id: u64,
        completed: u32,
        total: u32,
    },
    /// A long-running operation ended. Exactly one per operation.
    OperationFinished {
        operation_id: u64,
        outcome: OperationOutcome,
    },
    /// This subscriber fell behind and `count` older events were discarded.
    /// Re-read `snapshot()` rather than relying on the missed events.
    EventsDropped { count: u64 },
}

/// How a long-running operation ended.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum OperationOutcome {
    Completed {
        snapshot: CounterStateSnapshot,
    },
    /// Cancelled before its commit point; nothing was changed.
    Cancelled,
    /// Failed; nothing was changed.
    Failed {
        error: ForgeAppError,
    },
}

/// Fan-out of events to the current subscribers. Owned by the app instance.
pub(crate) struct EventHub {
    state: Mutex<HubState>,
}

struct HubState {
    closed: bool,
    subscribers: Vec<Arc<SubscriberQueue>>,
}

impl EventHub {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(HubState {
                closed: false,
                subscribers: Vec::new(),
            }),
        })
    }

    pub(crate) fn subscribe(self: &Arc<Self>) -> Result<CounterEventSubscription, ForgeAppError> {
        let queue = Arc::new(SubscriberQueue::new());
        let mut state = lock(&self.state);
        if state.closed {
            return Err(ForgeAppError::ShutDown);
        }
        state.subscribers.push(Arc::clone(&queue));
        Ok(CounterEventSubscription {
            queue,
            hub: Arc::downgrade(self),
        })
    }

    pub(crate) fn publish(&self, event: CounterEvent) {
        let state = lock(&self.state);
        if state.closed {
            return;
        }
        for subscriber in &state.subscribers {
            subscriber.push(event.clone());
        }
    }

    /// Ends every subscription. Events already queued are still delivered,
    /// then `next()` returns `None`. New subscriptions are refused.
    pub(crate) fn close(&self) {
        let subscribers = {
            let mut state = lock(&self.state);
            state.closed = true;
            std::mem::take(&mut state.subscribers)
        };
        for subscriber in subscribers {
            subscriber.finish(false);
        }
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        lock(&self.state).subscribers.len()
    }

    fn remove(&self, queue: &Arc<SubscriberQueue>) {
        lock(&self.state)
            .subscribers
            .retain(|s| !Arc::ptr_eq(s, queue));
    }
}

struct SubscriberQueue {
    state: Mutex<QueueState>,
}

struct QueueState {
    events: VecDeque<CounterEvent>,
    dropped: u64,
    finished: bool,
    waker: Option<Waker>,
}

impl SubscriberQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(QueueState {
                events: VecDeque::new(),
                dropped: 0,
                finished: false,
                waker: None,
            }),
        }
    }

    fn push(&self, event: CounterEvent) {
        let waker = {
            let mut state = lock(&self.state);
            if state.finished {
                return;
            }
            if state.events.len() >= QUEUE_CAPACITY {
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

    /// Marks the queue finished. `discard` drops undelivered events (the
    /// subscriber cancelled); otherwise they are still delivered (shutdown).
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

    fn poll_next(&self, cx: &mut Context<'_>) -> Poll<Option<CounterEvent>> {
        let mut state = lock(&self.state);
        if state.dropped > 0 {
            let count = std::mem::take(&mut state.dropped);
            return Poll::Ready(Some(CounterEvent::EventsDropped { count }));
        }
        if let Some(event) = state.events.pop_front() {
            return Poll::Ready(Some(event));
        }
        if state.finished {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// One subscriber's stream of [`CounterEvent`]s.
///
/// Owned by the host. Use it from a single consumer: concurrent `next()` calls
/// on the same subscription are not supported. The subscription ends when the
/// host calls `cancel()`, when it is released, or when the app shuts down.
#[derive(uniffi::Object)]
pub struct CounterEventSubscription {
    queue: Arc<SubscriberQueue>,
    hub: Weak<EventHub>,
}

#[uniffi::export]
impl CounterEventSubscription {
    /// The next event, or `None` once the subscription has ended.
    pub async fn next(&self) -> Option<CounterEvent> {
        NextEvent {
            queue: Arc::clone(&self.queue),
        }
        .await
    }

    /// Ends the subscription. A pending `next()` returns `None` straight away
    /// and undelivered events are discarded. Safe to call more than once.
    pub fn cancel(&self) {
        self.queue.finish(true);
        if let Some(hub) = self.hub.upgrade() {
            hub.remove(&self.queue);
        }
    }
}

impl Drop for CounterEventSubscription {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct NextEvent {
    queue: Arc<SubscriberQueue>,
}

impl Future for NextEvent {
    type Output = Option<CounterEvent>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.queue.poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::block_on;
    use std::thread;
    use std::time::Duration;

    fn changed(count: i64) -> CounterEvent {
        CounterEvent::StateChanged {
            snapshot: CounterStateSnapshot { count },
        }
    }

    #[test]
    fn delivers_in_order_to_every_subscriber() {
        let hub = EventHub::new();
        let a = hub.subscribe().unwrap();
        let b = hub.subscribe().unwrap();
        hub.publish(changed(1));
        hub.publish(changed(2));
        for sub in [&a, &b] {
            assert_eq!(block_on(sub.next()), Some(changed(1)));
            assert_eq!(block_on(sub.next()), Some(changed(2)));
        }
    }

    #[test]
    fn pending_next_wakes_when_an_event_arrives() {
        let hub = EventHub::new();
        let sub = hub.subscribe().unwrap();
        let publisher = Arc::clone(&hub);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            publisher.publish(changed(5));
        });
        assert_eq!(block_on(sub.next()), Some(changed(5)));
        handle.join().unwrap();
    }

    #[test]
    fn overflow_drops_oldest_and_reports_it_first() {
        let hub = EventHub::new();
        let sub = hub.subscribe().unwrap();
        let extra = 10;
        for i in 0..(QUEUE_CAPACITY as i64 + extra) {
            hub.publish(changed(i));
        }
        assert_eq!(
            block_on(sub.next()),
            Some(CounterEvent::EventsDropped {
                count: extra as u64
            })
        );
        // The newest QUEUE_CAPACITY events survive, oldest first.
        assert_eq!(block_on(sub.next()), Some(changed(extra)));
    }

    #[test]
    fn cancel_ends_a_pending_next_and_unregisters() {
        let hub = EventHub::new();
        let sub = Arc::new(hub.subscribe().unwrap());
        assert_eq!(hub.subscriber_count(), 1);
        let canceller = Arc::clone(&sub);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            canceller.cancel();
        });
        assert_eq!(block_on(sub.next()), None);
        handle.join().unwrap();
        assert_eq!(hub.subscriber_count(), 0);
        // Later events are not queued for a cancelled subscriber.
        hub.publish(changed(1));
        assert_eq!(block_on(sub.next()), None);
    }

    #[test]
    fn dropping_a_subscription_unregisters_it() {
        let hub = EventHub::new();
        let sub = hub.subscribe().unwrap();
        assert_eq!(hub.subscriber_count(), 1);
        drop(sub);
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[test]
    fn close_delivers_queued_events_then_ends() {
        let hub = EventHub::new();
        let sub = hub.subscribe().unwrap();
        hub.publish(changed(1));
        hub.close();
        assert_eq!(block_on(sub.next()), Some(changed(1)));
        assert_eq!(block_on(sub.next()), None);
        assert!(matches!(hub.subscribe(), Err(ForgeAppError::ShutDown)));
    }
}
