//! The serial command executor: one thread per core that owns the app's
//! state and runs commands one at a time, in arrival order.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};

use forge_runtime::lock;

pub(crate) type Job<S> = Box<dyn FnOnce(&mut S) + Send>;

/// Owns the command thread. Dropping the sender ends the thread once the
/// queue is drained.
pub(crate) struct Executor<S> {
    sender: Mutex<Option<mpsc::Sender<Job<S>>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    idle: Arc<(Mutex<usize>, Condvar)>,
}

impl<S: Send + 'static> Executor<S> {
    pub(crate) fn start(name: &str, mut state: S) -> Self {
        let (sender, receiver) = mpsc::channel::<Job<S>>();
        let idle = Arc::new((Mutex::new(0usize), Condvar::new()));
        let thread = {
            let idle = Arc::clone(&idle);
            thread::Builder::new()
                .name(format!("forge-app-{name}"))
                .spawn(move || {
                    for job in receiver {
                        // A panicking command must not take the core down
                        // with it; its reply reports the panic instead.
                        job(&mut state);
                        let (pending, done) = &*idle;
                        let mut pending = lock(pending);
                        *pending -= 1;
                        if *pending == 0 {
                            done.notify_all();
                        }
                    }
                })
                .expect("could not start the command thread")
        };
        Self {
            sender: Mutex::new(Some(sender)),
            thread: Mutex::new(Some(thread)),
            idle,
        }
    }

    /// Queues a job. Returns false once the executor is closed.
    pub(crate) fn submit(&self, job: Job<S>) -> bool {
        let sender = lock(&self.sender);
        let Some(sender) = sender.as_ref() else {
            return false;
        };
        *lock(&self.idle.0) += 1;
        if sender.send(job).is_err() {
            *lock(&self.idle.0) -= 1;
            return false;
        }
        true
    }

    /// Blocks until every accepted job has run. For tests and for hosts
    /// that must know saves are done (never call it on the main thread).
    pub(crate) fn wait_idle(&self) {
        let (pending, done) = &*self.idle;
        let mut pending = lock(pending);
        while *pending > 0 {
            pending = done.wait(pending).unwrap_or_else(|p| p.into_inner());
        }
    }
}

impl<S> Executor<S> {
    /// Accepts no more jobs; the thread finishes those already queued.
    pub(crate) fn close(&self) {
        lock(&self.sender).take();
    }

    pub(crate) fn join(&self) {
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

/// A one-shot reply from the command thread, awaited by the caller.
pub(crate) struct Reply<T> {
    slot: Arc<Mutex<ReplySlot<T>>>,
}

struct ReplySlot<T> {
    value: Option<thread::Result<T>>,
    waker: Option<Waker>,
}

pub(crate) struct ReplySender<T> {
    slot: Arc<Mutex<ReplySlot<T>>>,
}

pub(crate) fn reply<T>() -> (ReplySender<T>, Reply<T>) {
    let slot = Arc::new(Mutex::new(ReplySlot {
        value: None,
        waker: None,
    }));
    (
        ReplySender {
            slot: Arc::clone(&slot),
        },
        Reply { slot },
    )
}

impl<T> ReplySender<T> {
    pub(crate) fn send(self, value: thread::Result<T>) {
        let waker = {
            let mut slot = lock(&self.slot);
            slot.value = Some(value);
            slot.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> Future for Reply<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut slot = lock(&self.slot);
        match slot.value.take() {
            Some(Ok(value)) => Poll::Ready(value),
            Some(Err(panic)) => {
                drop(slot);
                std::panic::resume_unwind(panic)
            }
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
