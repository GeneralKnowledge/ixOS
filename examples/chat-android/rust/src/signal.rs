//! Two tiny synchronisation primitives that let the serial command thread hand
//! results to `async` callers without an async runtime.
//!
//! UniFFI polls exported futures from the foreign side (Swift or Kotlin) and
//! supplies the `Waker`; these types only need to store that waker and call it
//! when the value arrives. Nothing here spawns threads or executors.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

/// Locks a mutex, ignoring poisoning. The data these primitives guard is
/// never left half-updated by a panic (each critical section is a plain
/// assignment), so the value inside a poisoned lock is still consistent.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A value set once and awaited by any number of waiters.
pub(crate) struct Completion<T> {
    state: Mutex<CompletionState<T>>,
}

struct CompletionState<T> {
    value: Option<T>,
    wakers: Vec<Waker>,
}

impl<T: Clone> Completion<T> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(CompletionState {
                value: None,
                wakers: Vec::new(),
            }),
        })
    }

    /// Stores the value and wakes every waiter. Only the first call wins;
    /// later calls return `false` and change nothing.
    pub(crate) fn complete(&self, value: T) -> bool {
        let wakers = {
            let mut state = lock(&self.state);
            if state.value.is_some() {
                return false;
            }
            state.value = Some(value);
            std::mem::take(&mut state.wakers)
        };
        for waker in wakers {
            waker.wake();
        }
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn get(&self) -> Option<T> {
        lock(&self.state).value.clone()
    }

    pub(crate) fn wait(self: &Arc<Self>) -> CompletionFuture<T> {
        CompletionFuture {
            completion: Arc::clone(self),
        }
    }
}

pub(crate) struct CompletionFuture<T> {
    completion: Arc<Completion<T>>,
}

impl<T: Clone> Future for CompletionFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut state = lock(&self.completion.state);
        if let Some(value) = &state.value {
            return Poll::Ready(value.clone());
        }
        if !state.wakers.iter().any(|w| w.will_wake(cx.waker())) {
            state.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// Runs a future to completion on the current thread (tests only).
#[cfg(test)]
pub(crate) use forge_runtime::block_on;

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn completion_wakes_a_waiter_on_another_thread() {
        let completion = Completion::<u32>::new();
        let setter = Arc::clone(&completion);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            setter.complete(7);
        });
        assert_eq!(block_on(completion.wait()), 7);
        handle.join().unwrap();
    }

    #[test]
    fn only_the_first_completion_counts() {
        let completion = Completion::<u32>::new();
        assert!(completion.complete(1));
        assert!(!completion.complete(2));
        assert_eq!(completion.get(), Some(1));
        assert_eq!(block_on(completion.wait()), 1);
    }
}
