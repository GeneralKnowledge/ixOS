//! One answer being written: the state the engine's [`TokenSink`] fills and
//! the [`AssistantGeneration`] the screen reads.
//!
//! The engine may push a piece every few milliseconds. The screen does not
//! need that rate: `next()` hands over what has arrived since the last call,
//! at most about every [`DELIVERY_INTERVAL`], so SwiftUI redraws about 20
//! times a second however fast the model is.
//!
//! [`TokenSink`]: super::TokenSink

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use super::AssistantMessage;

/// The shortest time between two text deliveries to the screen.
pub const DELIVERY_INTERVAL: Duration = Duration::from_millis(50);

/// What the screen receives while an answer is written.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum AssistantUpdate {
    /// More of the answer: append `text`.
    Text { text: String },
    /// The answer ended (finished, stopped, refused or failed) and is
    /// saved. Replace the bubble with `message`. Always the last update.
    Finished { message: AssistantMessage },
}

pub(crate) struct Shared {
    started: Instant,
    cancelled: AtomicBool,
    /// The model started writing the user's side ("User: …"): the answer
    /// was cut there and is complete, not stopped.
    turn_ended: AtomicBool,
    /// Line starts that mean the model is writing another turn.
    turn_markers: Vec<String>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Everything pushed so far.
    text: String,
    /// The part of `text` the screen has not received yet starts here.
    delivered: usize,
    pieces: u32,
    first_piece: Option<Duration>,
    pieces_after_stop: u32,
    last_delivery: Option<Instant>,
    finished: Option<AssistantMessage>,
    finished_delivered: bool,
    waker: Option<Waker>,
}

/// What the sink saw, for the saved message and the log.
pub(crate) struct Outcome {
    pub text: String,
    pub pieces: u32,
    pub first_piece: Option<Duration>,
    pub elapsed: Duration,
    pub pieces_after_stop: u32,
}

impl Shared {
    pub fn new(turn_markers: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            cancelled: AtomicBool::new(false),
            turn_ended: AtomicBool::new(false),
            turn_markers,
            state: Mutex::new(State::default()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn push(&self, text: String) -> bool {
        let waker = {
            let mut state = self.lock();
            if self.is_cancelled() || self.turn_ended() || state.finished.is_some() {
                state.pieces_after_stop += 1;
                return false;
            }
            state.pieces += 1;
            if state.first_piece.is_none() {
                state.first_piece = Some(self.started.elapsed());
            }
            state.text.push_str(&text);
            // Small models sometimes go on to write the user's next message
            // and their own reply ("User: … / You: …"). The answer ends
            // where that starts; the engine is told to stop.
            if let Some(end) = super::characters::end_of_turn(&state.text, &self.turn_markers) {
                state.text.truncate(end);
                state.delivered = state.delivered.min(end);
                self.turn_ended.store(true, Ordering::SeqCst);
                let waker = state.waker.take();
                drop(state);
                if let Some(waker) = waker {
                    waker.wake();
                }
                return false;
            }
            // Wake the screen only when it may take the text now.
            let due = state
                .last_delivery
                .is_none_or(|at| at.elapsed() >= DELIVERY_INTERVAL);
            if due {
                state.waker.take()
            } else {
                None
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn turn_ended(&self) -> bool {
        self.turn_ended.load(Ordering::SeqCst)
    }

    /// Ends the generation: no more text is accepted, and the screen's
    /// next `next()` after the remaining text gets `Finished`.
    pub fn finish(&self, message: AssistantMessage) {
        let waker = {
            let mut state = self.lock();
            state.finished = Some(message);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub fn outcome(&self) -> Outcome {
        let state = self.lock();
        Outcome {
            text: state.text.clone(),
            pieces: state.pieces,
            first_piece: state.first_piece,
            elapsed: self.started.elapsed(),
            pieces_after_stop: state.pieces_after_stop,
        }
    }

    fn catch_up(&self) -> String {
        let mut state = self.lock();
        state.delivered = state.text.len();
        state.last_delivery = Some(Instant::now());
        state.text.clone()
    }

    fn poll_next(&self, cx: &mut Context<'_>) -> Poll<Option<AssistantUpdate>> {
        let mut state = self.lock();
        let pending = state.text.len() > state.delivered;
        let due = state
            .last_delivery
            .is_none_or(|at| at.elapsed() >= DELIVERY_INTERVAL);
        // Remaining text always goes out before `Finished`.
        if pending && (due || state.finished.is_some()) {
            let text = state.text[state.delivered..].to_string();
            state.delivered = state.text.len();
            state.last_delivery = Some(Instant::now());
            return Poll::Ready(Some(AssistantUpdate::Text { text }));
        }
        if let Some(message) = &state.finished {
            if state.finished_delivered {
                return Poll::Ready(None);
            }
            let message = message.clone();
            state.finished_delivered = true;
            return Poll::Ready(Some(AssistantUpdate::Finished { message }));
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// An answer being written. Read it with `next()` in a loop; stop it with
/// `cancel()`. Ending the loop early does not stop the answer: it is still
/// written and saved, and `AssistantCore::current_generation` finds it again.
#[derive(uniffi::Object)]
pub struct AssistantGeneration {
    pub(crate) shared: Arc<Shared>,
    pub(crate) conversation_id: String,
    pub(crate) message_id: String,
}

impl std::fmt::Debug for AssistantGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssistantGeneration")
            .field("conversation_id", &self.conversation_id)
            .field("message_id", &self.message_id)
            .finish()
    }
}

#[uniffi::export]
impl AssistantGeneration {
    /// The conversation the answer belongs to.
    pub fn conversation_id(&self) -> String {
        self.conversation_id.clone()
    }

    /// The answer's message id (its bubble, already in `messages`).
    pub fn message_id(&self) -> String {
        self.message_id.clone()
    }

    /// The whole answer so far, for a screen that opens while it is
    /// written; the next `Text` update continues after it.
    pub fn catch_up(&self) -> String {
        self.shared.catch_up()
    }

    /// The next update, or `None` after `Finished`. Text arrives at most
    /// about 20 times a second.
    pub async fn next(&self) -> Option<AssistantUpdate> {
        NextUpdate {
            shared: Arc::clone(&self.shared),
        }
        .await
    }

    /// Stops the answer (the Stop button). The engine is told at its next
    /// piece and must stop computing; the text so far is kept and the
    /// answer is saved as stopped. Idempotent.
    pub fn cancel(&self) {
        self.shared.cancel();
    }

    /// Whether the answer has ended.
    pub fn is_finished(&self) -> bool {
        self.shared.lock().finished.is_some()
    }
}

struct NextUpdate {
    shared: Arc<Shared>,
}

impl Future for NextUpdate {
    type Output = Option<AssistantUpdate>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.shared.poll_next(cx)
    }
}
