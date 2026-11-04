//! Pull-based delivery of [`ChatEvent`]s, like the Counter's
//! `CounterEventSubscription` (see `crate::events`), with a larger queue
//! because chat is the high-frequency example.
//!
//! The chat core emits events while holding its lock, so [`ChatEventHub::publish`]
//! only pushes onto each subscriber's bounded queue and wakes it. Nothing
//! calls into Swift or Kotlin from a Rust thread. When a queue is full the
//! oldest event is discarded and the next `next()` returns
//! [`ChatEvent::EventsDropped`] first.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use super::types::{ChatError, ChatEvent};
use crate::signal::lock;

/// Events buffered per chat subscription before the oldest are dropped.
pub const CHAT_QUEUE_CAPACITY: usize = 256;

pub(crate) struct ChatEventHub {
    state: Mutex<HubState>,
}

struct HubState {
    closed: bool,
    subscribers: Vec<Arc<Queue>>,
}

impl ChatEventHub {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(HubState {
                closed: false,
                subscribers: Vec::new(),
            }),
        })
    }

    pub(crate) fn subscribe(self: &Arc<Self>) -> Result<ChatEventSubscription, ChatError> {
        let queue = Arc::new(Queue::default());
        let mut state = lock(&self.state);
        if state.closed {
            return Err(ChatError::ShutDown);
        }
        state.subscribers.push(Arc::clone(&queue));
        Ok(ChatEventSubscription {
            queue,
            hub: Arc::downgrade(self),
        })
    }

    pub(crate) fn publish(&self, event: ChatEvent) {
        let state = lock(&self.state);
        if state.closed {
            return;
        }
        for subscriber in &state.subscribers {
            subscriber.push(event.clone());
        }
    }

    /// Ends every subscription. Queued events are still delivered, then
    /// `next()` returns `None`.
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

    #[cfg(test)]
    pub(crate) fn subscriber_count(&self) -> usize {
        lock(&self.state).subscribers.len()
    }

    fn remove(&self, queue: &Arc<Queue>) {
        lock(&self.state)
            .subscribers
            .retain(|s| !Arc::ptr_eq(s, queue));
    }
}

#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
}

#[derive(Default)]
struct QueueState {
    events: VecDeque<ChatEvent>,
    dropped: u64,
    finished: bool,
    waker: Option<Waker>,
}

impl Queue {
    fn push(&self, event: ChatEvent) {
        let waker = {
            let mut state = lock(&self.state);
            if state.finished {
                return;
            }
            if state.events.len() >= CHAT_QUEUE_CAPACITY {
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

    fn poll_next(&self, cx: &mut Context<'_>) -> Poll<Option<ChatEvent>> {
        let mut state = lock(&self.state);
        if state.dropped > 0 {
            let count = std::mem::take(&mut state.dropped);
            return Poll::Ready(Some(ChatEvent::EventsDropped { count }));
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

/// One subscriber's stream of [`ChatEvent`]s, for every room of the app
/// instance. Use it from a single consumer. It ends on `cancel()`, when it
/// is released, or when the app shuts down.
///
/// This is independent of `ForgeChatApp::subscribe(conversation_id)`, which
/// opens a room's live connection: cancelling this stream does not close any
/// room, and unsubscribing from a room does not end this stream.
#[derive(uniffi::Object)]
pub struct ChatEventSubscription {
    queue: Arc<Queue>,
    hub: Weak<ChatEventHub>,
}

#[uniffi::export]
impl ChatEventSubscription {
    /// The next event, or `None` once the subscription has ended.
    pub async fn next(&self) -> Option<ChatEvent> {
        NextEvent {
            queue: Arc::clone(&self.queue),
        }
        .await
    }

    /// Ends the subscription. A pending `next()` returns `None` at once and
    /// undelivered events are discarded. Idempotent.
    pub fn cancel(&self) {
        self.queue.finish(true);
        if let Some(hub) = self.hub.upgrade() {
            hub.remove(&self.queue);
        }
    }
}

impl Drop for ChatEventSubscription {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct NextEvent {
    queue: Arc<Queue>,
}

impl Future for NextEvent {
    type Output = Option<ChatEvent>;

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

    fn connected(i: usize) -> ChatEvent {
        ChatEvent::StreamConnected {
            conversation_id: format!("r{i}"),
        }
    }

    #[test]
    fn overflow_drops_oldest_and_reports_it_first() {
        let hub = ChatEventHub::new();
        let sub = hub.subscribe().unwrap();
        for i in 0..CHAT_QUEUE_CAPACITY + 3 {
            hub.publish(connected(i));
        }
        assert_eq!(
            block_on(sub.next()),
            Some(ChatEvent::EventsDropped { count: 3 })
        );
        assert_eq!(block_on(sub.next()), Some(connected(3)));
    }

    #[test]
    fn pending_next_wakes_and_cancel_ends_it() {
        let hub = ChatEventHub::new();
        let sub = Arc::new(hub.subscribe().unwrap());
        let publisher = Arc::clone(&hub);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            publisher.publish(connected(1));
        });
        assert_eq!(block_on(sub.next()), Some(connected(1)));
        handle.join().unwrap();

        let canceller = Arc::clone(&sub);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            canceller.cancel();
        });
        assert_eq!(block_on(sub.next()), None);
        handle.join().unwrap();
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[test]
    fn close_delivers_queued_then_ends_and_refuses_new() {
        let hub = ChatEventHub::new();
        let sub = hub.subscribe().unwrap();
        hub.publish(connected(1));
        hub.close();
        assert_eq!(block_on(sub.next()), Some(connected(1)));
        assert_eq!(block_on(sub.next()), None);
        assert!(matches!(hub.subscribe(), Err(ChatError::ShutDown)));
    }

    #[test]
    fn dropping_unregisters() {
        let hub = ChatEventHub::new();
        drop(hub.subscribe().unwrap());
        assert_eq!(hub.subscriber_count(), 0);
    }
}
