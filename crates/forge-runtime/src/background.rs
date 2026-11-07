//! Async commands without an async runtime.
//!
//! UniFFI polls an exported future from Swift or Kotlin and supplies the
//! waker. [`background`] runs a closure on a new thread and returns a future
//! that completes, waking that poller, when the closure returns.

use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use forge_net::CancelHandle;

use crate::shell::lock;

struct Slot<T> {
    result: Option<std::thread::Result<T>>,
    done: bool,
    waker: Option<Waker>,
}

/// The future [`background`] returns. Dropping it does not stop the work
/// (threads cannot be killed); the result is then discarded. For work made
/// with [`background_cancellable`], dropping it before the work finishes
/// also cancels the work's [`CancelHandle`].
pub struct Background<T> {
    slot: Arc<Mutex<Slot<T>>>,
    cancel_on_drop: Option<CancelHandle>,
}

impl<T> Drop for Background<T> {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel_on_drop {
            if !lock(&self.slot).done {
                cancel.cancel();
            }
        }
    }
}

impl<T> Future for Background<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut slot = lock(&self.slot);
        match slot.result.take() {
            Some(Ok(value)) => return Poll::Ready(value),
            Some(Err(panic)) => {
                drop(slot);
                std::panic::resume_unwind(panic)
            }
            None => {}
        }
        assert!(!slot.done, "Background polled after it completed");
        slot.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// Runs `work` on a new thread named `forge-<name>` and resolves with its
/// result.
///
/// ```
/// # use forge_runtime::{background, block_on};
/// let answer = block_on(background("answer", || 6 * 7));
/// assert_eq!(answer, 42);
/// ```
///
/// A panic in `work` is caught and re-raised when the future is polled, so
/// UniFFI reports it to the host as a panic of that call rather than leaving
/// the host waiting forever. If the OS refuses a new thread, `work` runs on
/// the calling thread instead.
pub fn background<T, F>(name: &str, work: F) -> Background<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let slot = Arc::new(Mutex::new(Slot {
        result: None,
        done: false,
        waker: None,
    }));
    let finish = {
        let slot = Arc::clone(&slot);
        move |result: std::thread::Result<T>| {
            let waker = {
                let mut slot = lock(&slot);
                slot.done = true;
                slot.result = Some(result);
                slot.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    };
    let job = move || finish(std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)));
    let job = Arc::new(Mutex::new(Some(job)));
    let spawned = {
        let job = Arc::clone(&job);
        thread::Builder::new()
            .name(format!("forge-{name}"))
            .spawn(move || {
                if let Some(job) = lock(&job).take() {
                    job();
                }
            })
    };
    if spawned.is_err() {
        if let Some(job) = lock(&job).take() {
            job();
        }
    }
    Background {
        slot,
        cancel_on_drop: None,
    }
}

/// [`background`] for work that can be stopped part way, such as an HTTP
/// request ([`forge_net::HttpClient::send_cancellable`]). `work` receives
/// `cancel`; it is cancelled when `cancel` is (an explicit cancel from the
/// host, or `shutdown`), and when the returned future is dropped before the
/// work finishes, which is what Kotlin does when the calling coroutine is
/// cancelled. Swift does not drop the future when its `Task` is cancelled
/// (UniFFI 0.32), so a Swift host cancels through an exported `cancel()`.
///
/// ```
/// # use forge_runtime::{background_cancellable, block_on, CancelHandle};
/// let cancel = CancelHandle::new();
/// cancel.cancel();
/// let stopped = block_on(background_cancellable("wait", cancel, |cancel| {
///     cancel.wait_timeout(std::time::Duration::from_secs(60))
/// }));
/// assert!(stopped);
/// ```
pub fn background_cancellable<T, F>(name: &str, cancel: CancelHandle, work: F) -> Background<T>
where
    T: Send + 'static,
    F: FnOnce(&CancelHandle) -> T + Send + 'static,
{
    let for_work = cancel.clone();
    let mut future = background(name, move || work(&for_work));
    future.cancel_on_drop = Some(cancel);
    future
}

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs a future to completion on the current thread. For tests and for
/// synchronous wrappers; never call it from inside an exported `async fn`.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dropping_a_cancellable_future_cancels_the_work() {
        let cancel = CancelHandle::new();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let future = background_cancellable("drop-cancels", cancel.clone(), move |cancel| {
            let stopped = cancel.wait_timeout(std::time::Duration::from_secs(30));
            done_tx.send(stopped).unwrap();
        });
        drop(future);
        let stopped = done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the work ended promptly");
        assert!(stopped);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn a_finished_cancellable_future_does_not_cancel_on_drop() {
        let cancel = CancelHandle::new();
        let mut future = Box::pin(background_cancellable("finishes", cancel.clone(), |_| 7));
        assert_eq!(block_on(future.as_mut()), 7);
        drop(future);
        assert!(!cancel.is_cancelled());
    }

    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn resolves_with_the_closure_result_off_the_calling_thread() {
        let caller = thread::current().id();
        let (value, worker) = block_on(background("t", || (7, thread::current().id())));
        assert_eq!(value, 7);
        assert_ne!(worker, caller);
    }

    #[test]
    fn many_commands_run_concurrently() {
        let started = Instant::now();
        let futures: Vec<_> = (0..8)
            .map(|i| {
                background("sleep", move || {
                    thread::sleep(Duration::from_millis(200));
                    i
                })
            })
            .collect();
        let sum: i32 = futures.into_iter().map(block_on).sum();
        assert_eq!(sum, 28);
        assert!(started.elapsed() < Duration::from_millis(1500));
    }

    #[test]
    fn a_panic_reaches_the_awaiting_caller() {
        let outcome = std::panic::catch_unwind(|| block_on(background("boom", || panic!("boom"))));
        assert!(outcome.is_err());
    }

    #[test]
    fn dropping_the_future_discards_the_result() {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(background("dropped", move || tx.send(1).unwrap()));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(1));
    }
}
