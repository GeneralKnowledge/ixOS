//! The on-device assistant (milestone L1 in docs/local-ai.md): private
//! conversations with a model that runs on the iPhone, with or without a
//! chat account.
//!
//! - **Rust owns the conversation**: the characters and their prompt
//!   harness ([`characters`]), the history in `assistant.db` ([`store`]),
//!   trimming to the model's context, when an answer stops, and what is
//!   saved.
//! - **Swift owns the model**: an engine object implementing
//!   [`LocalModelEngine`] (Apple Foundation Models in L1) turns a
//!   [`GenerateRequest`] into text and pushes it into a [`TokenSink`].
//! - **One answer at a time.** `send` saves the user's message and an empty
//!   answer marked `Streaming`, then runs the engine on a thread of its own
//!   and returns an [`AssistantGeneration`] at once; the screen reads it
//!   with `next()` and stops it with `cancel()`.
//!
//! Everything here moves to a `forge-ai` framework crate once the device
//! runs have settled the design (milestone L5).

mod characters;
mod engine;
mod generation;
mod store;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub use characters::AssistantCharacter;
pub use engine::{
    AiError, EngineInfo, GenerateRequest, LocalModelEngine, PromptRole, PromptTurn, TokenSink,
};
pub use generation::{AssistantGeneration, AssistantUpdate};

use generation::Shared;
use store::{ConversationRow, Store};

/// Who wrote a message.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssistantRole {
    User,
    Assistant,
}

/// Where an answer stands. User messages are always `Done`.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssistantMessageState {
    Done,
    /// Being written now.
    Streaming,
    /// The user pressed Stop; the text so far is kept.
    Stopped,
    /// The app ended while it was being written.
    Interrupted,
    /// The model's safety rules declined; `error` says why.
    Refused,
    /// The engine failed; `error` says why.
    Failed,
}

/// How an answer was made, for the line under it and the log.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AssistantStats {
    pub engine_id: String,
    /// From sending to the first text.
    pub first_text_ms: Option<u64>,
    /// From sending to the end.
    pub duration_ms: u64,
    /// Pieces the engine pushed (about one per token for most engines;
    /// Foundation Models pushes larger pieces).
    pub pieces: u32,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AssistantMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: AssistantRole,
    pub text: String,
    pub state: AssistantMessageState,
    /// Why it was refused or failed.
    pub error: Option<String>,
    /// Unix ms.
    pub created_at: i64,
    /// Answers only, once they have ended.
    pub stats: Option<AssistantStats>,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AssistantConversation {
    pub id: String,
    pub character: AssistantCharacter,
    /// Unix ms.
    pub created_at: i64,
    /// Unix ms of the newest message.
    pub updated_at: i64,
    /// For the list's preview line.
    pub last_message: Option<AssistantMessage>,
}

#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AssistantError {
    /// The app has not installed a model engine (`set_engine`).
    #[error("no model engine is installed")]
    NoEngine,
    /// The engine cannot run now; `reason` says what the user can do.
    #[error("the model is unavailable: {reason}")]
    Unavailable { reason: String },
    /// An answer is being written; stop it or wait.
    #[error("an answer is still being written")]
    Busy,
    #[error("not found: {what}")]
    NotFound { what: String },
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("storage error: {reason}")]
    Storage { reason: String },
    #[error("the assistant has been shut down")]
    ShutDown,
}

/// The longest message the user can send, in characters.
pub const MAX_MESSAGE_CHARS: usize = 4_000;

/// The assistant. `ChatCore::assistant()` hands out the app's one instance.
#[derive(uniffi::Object)]
pub struct AssistantCore {
    inner: Arc<Inner>,
}

struct Inner {
    store: Mutex<Store>,
    engine: RwLock<Option<Arc<dyn LocalModelEngine>>>,
    current: Mutex<Option<Arc<AssistantGeneration>>>,
    shut_down: AtomicBool,
}

#[uniffi::export]
impl AssistantCore {
    /// The characters a new conversation can use: the plain assistant
    /// first, then the built-in test characters.
    pub fn characters(&self) -> Vec<AssistantCharacter> {
        characters::all()
    }

    /// Installs the model engine (the app does this at launch). Replaces
    /// any previous one; an answer being written finishes on the old one.
    pub fn set_engine(&self, engine: Arc<dyn LocalModelEngine>) {
        *self.inner.engine.write().unwrap_or_else(|p| p.into_inner()) = Some(engine);
    }

    /// The installed engine and whether it can run now, or `None` before
    /// `set_engine`.
    pub fn engine_info(&self) -> Option<EngineInfo> {
        self.inner.engine().map(|engine| engine.describe())
    }

    /// Every conversation, most recent first.
    pub fn conversations(&self) -> Result<Vec<AssistantConversation>, AssistantError> {
        let store = self.inner.store();
        store
            .conversations()?
            .into_iter()
            .map(|row| {
                let last_message = store.last_message(&row.id)?;
                Ok(conversation(row, last_message))
            })
            .collect()
    }

    /// Starts a conversation with `character_id`. A character with an
    /// opening line has already said it.
    pub fn start_conversation(
        &self,
        character_id: String,
    ) -> Result<AssistantConversation, AssistantError> {
        self.inner.ensure_running()?;
        if !characters::exists(&character_id) {
            return Err(AssistantError::NotFound {
                what: format!("character {character_id}"),
            });
        }
        let now = now_ms();
        let row = ConversationRow {
            id: new_id("conv"),
            character_id: character_id.clone(),
            created_at: now,
            updated_at: now,
        };
        let store = self.inner.store();
        store.insert_conversation(&row)?;
        let opening = characters::find(&character_id)
            .opening_line
            .map(|line| AssistantMessage {
                id: new_id("msg"),
                conversation_id: row.id.clone(),
                role: AssistantRole::Assistant,
                text: line.to_string(),
                state: AssistantMessageState::Done,
                error: None,
                created_at: now,
                stats: None,
            });
        if let Some(message) = &opening {
            store.insert_message(message)?;
        }
        Ok(conversation(row, opening))
    }

    /// Deletes a conversation and its messages from this iPhone, stopping
    /// its answer if one is being written.
    pub fn delete_conversation(&self, conversation_id: String) -> Result<(), AssistantError> {
        if let Some(current) = self.current_generation() {
            if current.conversation_id == conversation_id {
                current.cancel();
            }
        }
        if self.inner.store().delete_conversation(&conversation_id)? {
            Ok(())
        } else {
            Err(not_found(&conversation_id))
        }
    }

    /// A conversation's messages, oldest first. An answer being written is
    /// there as `Streaming` with the text saved so far (none until it ends);
    /// `current_generation` has the live text.
    pub fn messages(
        &self,
        conversation_id: String,
    ) -> Result<Vec<AssistantMessage>, AssistantError> {
        let store = self.inner.store();
        if store.conversation(&conversation_id)?.is_none() {
            return Err(not_found(&conversation_id));
        }
        store.messages(&conversation_id)
    }

    /// Sends `text` and starts the answer. Saves the message and an empty
    /// answer (`Streaming`) before it returns; read the answer from the
    /// returned generation. Fails with `Busy` while another answer is being
    /// written, and with `Unavailable` (saving nothing) when the engine
    /// cannot run now.
    pub fn send(
        &self,
        conversation_id: String,
        text: String,
    ) -> Result<Arc<AssistantGeneration>, AssistantError> {
        self.inner.ensure_running()?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(AssistantError::InvalidInput {
                reason: "the message is empty".into(),
            });
        }
        if text.chars().count() > MAX_MESSAGE_CHARS {
            return Err(AssistantError::InvalidInput {
                reason: format!("messages are limited to {MAX_MESSAGE_CHARS} characters"),
            });
        }
        let engine = self.inner.engine().ok_or(AssistantError::NoEngine)?;
        let info = engine.describe();
        if !info.available {
            return Err(AssistantError::Unavailable {
                reason: info
                    .unavailable_reason
                    .unwrap_or_else(|| format!("{} is not available", info.name)),
            });
        }

        let mut current = self.inner.current();
        if current.as_ref().is_some_and(|g| !g.is_finished()) {
            return Err(AssistantError::Busy);
        }
        let (request, answer, markers) = {
            let store = self.inner.store();
            let row = store
                .conversation(&conversation_id)?
                .ok_or_else(|| not_found(&conversation_id))?;
            let character = characters::find(&row.character_id);
            let markers = character.turn_markers();
            let history = store
                .messages(&conversation_id)?
                .into_iter()
                .filter_map(|m| prompt_turn(m, &markers))
                .collect();
            let request = characters::build_request(character, history, &text, info.context_tokens);
            let now = now_ms();
            store.insert_message(&AssistantMessage {
                id: new_id("msg"),
                conversation_id: conversation_id.clone(),
                role: AssistantRole::User,
                text,
                state: AssistantMessageState::Done,
                error: None,
                created_at: now,
                stats: None,
            })?;
            let answer = AssistantMessage {
                id: new_id("msg"),
                conversation_id: conversation_id.clone(),
                role: AssistantRole::Assistant,
                text: String::new(),
                state: AssistantMessageState::Streaming,
                error: None,
                created_at: now,
                stats: None,
            };
            store.insert_message(&answer)?;
            (request, answer, markers)
        };

        let generation = Arc::new(AssistantGeneration {
            shared: Shared::new(markers),
            conversation_id,
            message_id: answer.id.clone(),
        });
        *current = Some(Arc::clone(&generation));
        drop(current);
        self.inner
            .spawn(engine, info.id, request, answer, &generation);
        Ok(generation)
    }

    /// The answer being written now, if any, so a screen that opens
    /// mid-answer can follow it (`catch_up`, then `next`).
    pub fn current_generation(&self) -> Option<Arc<AssistantGeneration>> {
        self.inner
            .current()
            .as_ref()
            .filter(|g| !g.is_finished())
            .cloned()
    }

    /// Stops any answer and refuses further sends. Called by
    /// `ChatCore::shutdown`. Idempotent.
    pub fn shutdown(&self) {
        self.inner.shut_down.store(true, Ordering::SeqCst);
        if let Some(current) = self.current_generation() {
            current.cancel();
        }
    }
}

impl AssistantCore {
    /// Opens (or creates) `assistant.db` at `path`. Answers left
    /// `Streaming` by the last run become `Interrupted`.
    pub fn open(path: &Path) -> Result<Arc<Self>, AssistantError> {
        Ok(Self::with_store(Store::open(path)?))
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Arc<Self> {
        Self::with_store(Store::in_memory())
    }

    fn with_store(store: Store) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                store: Mutex::new(store),
                engine: RwLock::new(None),
                current: Mutex::new(None),
                shut_down: AtomicBool::new(false),
            }),
        })
    }
}

impl Inner {
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn current(&self) -> MutexGuard<'_, Option<Arc<AssistantGeneration>>> {
        self.current.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn engine(&self) -> Option<Arc<dyn LocalModelEngine>> {
        self.engine
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn ensure_running(&self) -> Result<(), AssistantError> {
        if self.shut_down.load(Ordering::SeqCst) {
            Err(AssistantError::ShutDown)
        } else {
            Ok(())
        }
    }

    /// Runs the engine on a thread of its own (it awaits Swift, which calls
    /// back on its own threads), then saves the answer and ends the
    /// generation.
    fn spawn(
        self: &Arc<Self>,
        engine: Arc<dyn LocalModelEngine>,
        engine_id: String,
        request: GenerateRequest,
        answer: AssistantMessage,
        generation: &Arc<AssistantGeneration>,
    ) {
        let inner = Arc::clone(self);
        let shared = Arc::clone(&generation.shared);
        let spawned = std::thread::Builder::new()
            .name("assistant-generate".into())
            .spawn({
                let shared = Arc::clone(&shared);
                let answer = answer.clone();
                let engine_id = engine_id.clone();
                move || {
                    let sink = Arc::new(TokenSink {
                        shared: Arc::clone(&shared),
                    });
                    let result = forge_runtime::block_on(engine.generate(request, sink));
                    inner.finish(&shared, answer, engine_id, result);
                }
            });
        if let Err(err) = spawned {
            self.finish(
                &shared,
                answer,
                engine_id,
                Err(AiError::Engine {
                    reason: format!("could not start: {err}"),
                }),
            );
        }
    }

    fn finish(
        &self,
        shared: &Shared,
        mut answer: AssistantMessage,
        engine_id: String,
        result: Result<(), AiError>,
    ) {
        let outcome = shared.outcome();
        let stopped = shared.is_cancelled();
        let (state, error) = match result {
            // A stopped answer may end with Ok or with the engine's own
            // cancellation error; either way it is "stopped".
            _ if stopped => (AssistantMessageState::Stopped, None),
            // Cut where the model began writing the user's side: complete.
            _ if shared.turn_ended() => (AssistantMessageState::Done, None),
            Ok(()) => (AssistantMessageState::Done, None),
            Err(AiError::Refused { reason }) => (AssistantMessageState::Refused, Some(reason)),
            Err(err) => (AssistantMessageState::Failed, Some(err.to_string())),
        };
        answer.text = outcome.text;
        answer.state = state;
        answer.error = error;
        answer.stats = Some(AssistantStats {
            engine_id: engine_id.clone(),
            first_text_ms: outcome.first_piece.map(|d| d.as_millis() as u64),
            duration_ms: outcome.elapsed.as_millis() as u64,
            pieces: outcome.pieces,
        });
        if let Err(err) = self.store().finish_message(&answer) {
            forge_runtime::logging::warn!(target: "assistant", "could not save an answer: {err}");
        }
        // Timings only: never the conversation's text. At warn level because
        // the iPhone's syslog relay drops info lines, and `forge device logs
        // --grep assistant` on Linux is how device runs record these.
        forge_runtime::logging::warn!(
            target: "assistant",
            "answer {:?} engine={engine_id} first_text_ms={:?} duration_ms={} pieces={} pieces_after_stop={}",
            state,
            outcome.first_piece.map(|d| d.as_millis()),
            outcome.elapsed.as_millis(),
            outcome.pieces,
            outcome.pieces_after_stop,
        );
        shared.finish(answer);
    }
}

fn conversation(
    row: ConversationRow,
    last_message: Option<AssistantMessage>,
) -> AssistantConversation {
    AssistantConversation {
        character: characters::find(&row.character_id).public(),
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_message,
    }
}

/// What the model reads of an earlier message: everything the user wrote,
/// and the assistant's answers that have text (a stopped answer counts; a
/// failed or empty one does not).
fn prompt_turn(message: AssistantMessage, markers: &[String]) -> Option<PromptTurn> {
    let role = match message.role {
        AssistantRole::User => PromptRole::User,
        AssistantRole::Assistant => {
            let usable = matches!(
                message.state,
                AssistantMessageState::Done
                    | AssistantMessageState::Stopped
                    | AssistantMessageState::Interrupted
            );
            if !usable {
                return None;
            }
            PromptRole::Assistant
        }
    };
    // Answers saved before turns were cut may still hold invented
    // "User: …" lines: the model must not read them as real.
    let mut text = message.text;
    if role == PromptRole::Assistant {
        if let Some(end) = characters::end_of_turn(&text, markers) {
            text.truncate(end);
        }
        if text.trim().is_empty() {
            return None;
        }
    }
    Some(PromptTurn { role, text })
}

fn not_found(conversation_id: &str) -> AssistantError {
    AssistantError::NotFound {
        what: format!("conversation {conversation_id}"),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Unique on this device: time, process-wide counter.
fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos:x}-{n:x}")
}
