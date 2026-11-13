//! The Chat app's Rust core: the account, the chat engine and the app
//! lifecycle, behind one object the SwiftUI host creates once
//! ([`ChatCore`]).
//!
//! - **Account.** `sign_up` creates an account on the chat server and keeps
//!   it, token included, in the iOS Keychain through `forge-services`
//!   ([`credentials`]). The token never crosses into Swift.
//! - **Chat.** `chat_core::ChatClient` does the work: the per-room live
//!   streams, the store, sending and retrying. Each account has its own
//!   store under the storage directory.
//! - **Lifecycle.** The host reports scene phases with `set_phase`; in the
//!   background every live stream stops (iOS suspends the app within
//!   seconds anyway), and on resume they restart and catch up.
//! - **Assistant.** A private assistant that runs on the iPhone
//!   ([`assistant`], `ChatCore::assistant`). It has its own store and works
//!   signed in or out.
//!
//! Network commands are `async` and run on worker threads
//! ([`forge_runtime::background`]). They are not cancelled by the host: a
//! send that has started finishes, and the event stream reports how it
//! ended (see `docs/chat-demo.md`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use chat_core::{ChatClient, ChatConfig, ChatStore, HttpRepo};
use forge_runtime::{background, EventHub, Lifecycle, Phase};

pub mod assistant;
pub mod credentials;
mod types;

pub use assistant::AssistantCore;
pub use credentials::{Credentials, KeychainCredentials, MemoryCredentials, StoredAccount};
pub use types::*;

// No namespace argument: UniFFI takes the library's name (chat_app), so this
// crate keeps its own bindings when another app links it.
uniffi::setup_scaffolding!();

// The lifecycle phase the host reports and the event stream object
// (`ChatEvents::subscribe(&hub)`). See `forge_runtime::app_shell!`.
forge_runtime::app_shell! {
    phase: AppPhase,
    events: ChatEvents(ChatEvent::EventsDropped),
}

// Native services the host implements (Keychain, directories, notifications,
// permissions, deep links), as `crate::native`. The app installs them with
// `ForgeServices.install()`; see docs/native-services.md.
forge_services::native_services!(audio, biometrics, http);

// `forge_log` for the host: Swift's `ForgeLog("Category").warn(...)` writes
// into the same log as Rust (system log, recent records, `forge device`).
forge_runtime::log_export!();

/// Events queued per consumer before older ones are dropped
/// (`ChatEvent::EventsDropped`).
pub const EVENT_QUEUE_CAPACITY: usize = 512;

/// The app instance. Create one at launch and share it. Thread-safe.
#[derive(uniffi::Object)]
pub struct ChatCore {
    storage_directory: PathBuf,
    credentials: Arc<dyn Credentials>,
    session: Mutex<Option<Session>>,
    events: EventHub<ChatEvent>,
    lifecycle: Lifecycle,
    shut_down: AtomicBool,
    assistant: Arc<AssistantCore>,
}

/// The signed-in account and its chat engine.
struct Session {
    account: ChatAccount,
    client: ChatClient,
}

#[uniffi::export]
impl ChatCore {
    /// Opens the app with its account in the iOS Keychain. Needs
    /// `ForgeServices.install()` first; without it this fails with
    /// `UnsupportedOnHost`. `storage_directory` (created if missing) holds
    /// each account's chats.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, ChatError> {
        let credentials = KeychainCredentials::installed()?;
        let core =
            Self::open(PathBuf::from(storage_directory), Arc::new(credentials)).map(Arc::new)?;
        core.handle_notification_actions();
        Ok(core)
    }

    /// Like `new`, but the account is only kept in memory, so it is gone at
    /// the next launch. For tests and `swift run` on a Mac, where there is
    /// no app Keychain; never for the app.
    #[uniffi::constructor]
    pub fn with_memory_credentials(storage_directory: String) -> Result<Arc<Self>, ChatError> {
        Self::open(
            PathBuf::from(storage_directory),
            Arc::new(MemoryCredentials::default()),
        )
        .map(Arc::new)
    }

    /// The signed-in account, if any. Known at once at launch: the app works
    /// offline from its store without asking the server.
    pub fn account(&self) -> Option<ChatAccount> {
        self.lock().as_ref().map(|s| s.account.clone())
    }

    /// Creates an account named `display_name` on the chat server at
    /// `server_url`, stores it in the Keychain and switches to it. A
    /// previous account on this device is signed out first (its chats are
    /// removed from the device). Emits `AccountChanged`.
    pub async fn sign_up(
        &self,
        server_url: String,
        display_name: String,
    ) -> Result<ChatAccount, ChatError> {
        self.ensure_running()?;
        let server_url = normalise_server_url(&server_url)?;
        let display_name = chat_core::validate_display_name(&display_name)?;
        let registered = {
            let server_url = server_url.clone();
            background("chat-sign-up", move || {
                server_repo(server_url)
                    .register(&display_name)
                    .map_err(ChatError::from)
            })
            .await?
        };
        let stored = StoredAccount {
            server_url,
            user_id: registered.user_id,
            display_name: registered.display_name,
            token: registered.token,
        };
        self.ensure_running()?;
        self.sign_out_locked(&mut self.lock(), false)?;
        self.credentials.save(&stored)?;
        let mut session = self.lock();
        let opened = self.open_session(&stored)?;
        let account = opened.account.clone();
        *session = Some(opened);
        drop(session);
        self.events.publish(ChatEvent::AccountChanged {
            account: Some(account.clone()),
        });
        Ok(account)
    }

    /// Asks the server who the stored token belongs to. Fails with
    /// `Unauthorized` (and emits `AccountRejected`) when the server no
    /// longer knows it, and with `Network` when it cannot be reached (the
    /// app keeps working from its store).
    pub async fn check_account(&self) -> Result<ChatAccount, ChatError> {
        self.ensure_running()?;
        let stored = self.credentials.load()?.ok_or(ChatError::NotSignedIn)?;
        let account = stored.public();
        let checked = background("chat-check-account", move || {
            server_repo(&stored.server_url)
                .with_token(&stored.token)
                .identity()
                .map_err(ChatError::from)
        })
        .await;
        match checked {
            Ok(identity) if identity.user_id == account.user_id => Ok(account),
            Ok(identity) => Err(self.rejected(ChatError::Unauthorized {
                reason: format!(
                    "the token belongs to {}, not {}",
                    identity.user_id, account.user_id
                ),
            })),
            Err(err) => Err(self.rejected(err)),
        }
    }

    /// Forgets the account: removes it from the Keychain and its chats from
    /// the device. The account itself stays on the server. Emits
    /// `AccountChanged`. Idempotent.
    pub fn sign_out(&self) -> Result<(), ChatError> {
        self.sign_out_locked(&mut self.lock(), true)
    }

    /// The on-device assistant. Works without an account, and keeps its
    /// conversations when the account signs out.
    pub fn assistant(&self) -> Arc<AssistantCore> {
        Arc::clone(&self.assistant)
    }

    /// A new event stream. Each consumer takes its own and sees every event
    /// from now on.
    pub fn events(&self) -> Result<Arc<ChatEvents>, ChatError> {
        ChatEvents::subscribe(&self.events).ok_or(ChatError::ShutDown)
    }

    /// Lists conversations from the server, prefetching new messages for
    /// rooms that are not open (they count as unread).
    pub async fn list_conversations(&self) -> Result<Vec<ChatConversation>, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-list", move || {
            client
                .list_conversations()
                .map(|list| {
                    list.into_iter()
                        .map(|c| types::conversation(c, &user_id))
                        .collect()
                })
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Creates a conversation on the server. It is added to the list
    /// (`ConversationUpdated`) and returned, ready to open.
    pub async fn create_conversation(&self, title: String) -> Result<ChatConversation, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-create", move || {
            client
                .create_conversation(&title)
                .map(|c| types::conversation(c, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Marks everything in the room read, here at once and then on the
    /// server (other devices and other people see it). Clears "marked
    /// unread". An open room is kept read without this.
    pub async fn mark_read(&self, conversation_id: String) -> Result<(), ChatError> {
        let client = self.client()?;
        let result = background("chat-read", move || {
            client.mark_read(&conversation_id).map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Shows the room as unread until it is opened or marked read (`true`),
    /// or stops doing so. This device only.
    pub fn set_marked_unread(
        &self,
        conversation_id: String,
        marked: bool,
    ) -> Result<(), ChatError> {
        self.client()?
            .set_marked_unread(&conversation_id, marked)
            .map_err(Into::into)
    }

    /// Whether Chat asks for Face ID, Touch ID or the passcode when it
    /// opens and after a minute in the background. This device only.
    pub fn app_lock(&self) -> bool {
        self.storage_directory.join(APP_LOCK_FILE).exists()
    }

    /// Turns the app lock on or off. Turning it on is refused when this
    /// iPhone has neither biometrics nor a passcode, since the lock could
    /// never open.
    pub fn set_app_lock(&self, enabled: bool) -> Result<(), ChatError> {
        let path = self.storage_directory.join(APP_LOCK_FILE);
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(ChatError::Storage {
                    reason: format!("turn off the lock: {e}"),
                }),
                _ => Ok(()),
            };
        }
        if app_lock_method().is_none() {
            return Err(ChatError::InvalidInput {
                reason: "set a passcode on this iPhone first".into(),
            });
        }
        std::fs::write(&path, b"").map_err(|e| ChatError::Storage {
            reason: format!("turn on the lock: {e}"),
        })
    }

    /// Mutes the room (no notifications) or unmutes it. This device only.
    pub fn set_muted(&self, conversation_id: String, muted: bool) -> Result<(), ChatError> {
        self.client()?
            .set_muted(&conversation_id, muted)
            .map_err(Into::into)
    }

    /// Fetches the room's messages newer than what is stored, merges them
    /// and returns the room's full stored list, oldest first.
    pub async fn sync_messages(
        &self,
        conversation_id: String,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-sync", move || {
            client
                .sync_messages(&conversation_id)
                .map(|list| {
                    list.into_iter()
                        .map(|m| types::message(m, &user_id))
                        .collect()
                })
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Sends a message. It is announced as `Sending` at once, then becomes
    /// `Sent` (`MessageStatusChanged`). If the server cannot be reached it
    /// stays `Sending` in the outbox and goes out by itself later, even
    /// after a relaunch; only a refusal by the server makes it `Failed`.
    /// Resolves with the message as it then is, or the error that failed it.
    /// Clears the conversation's draft.
    pub async fn send_message(
        &self,
        conversation_id: String,
        body: String,
    ) -> Result<ChatMessage, ChatError> {
        self.send_reply(conversation_id, body, None).await
    }

    /// `send_message` as an answer to `reply_to`, a message in the same
    /// conversation.
    pub async fn send_reply(
        &self,
        conversation_id: String,
        body: String,
        reply_to: Option<String>,
    ) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-send", move || {
            client
                .send_reply(&conversation_id, &body, reply_to.as_deref())
                .map(|m| types::message(m, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// `send_reply` with photos or documents (at most 10, each up to 25 MB).
    /// Rust copies each file into the app's attachment store before this
    /// returns its first event, so the originals may be temporary. The body
    /// may be empty. Offline, the files wait in the outbox with the message.
    pub async fn send_files(
        &self,
        conversation_id: String,
        body: String,
        reply_to: Option<String>,
        files: Vec<ChatOutgoingFile>,
    ) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-send-files", move || {
            let files: Vec<chat_core::OutgoingFile> = files.into_iter().map(Into::into).collect();
            client
                .send_with_files(&conversation_id, &body, reply_to.as_deref(), &files)
                .map(|m| types::message(m, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// The local file of an attachment, if it is on this iPhone already
    /// (sent from here, or fetched before). Its name is the attachment id,
    /// without an extension.
    pub fn attachment_file(&self, attachment_id: String) -> Option<String> {
        self.client()
            .ok()?
            .attachment_file(&attachment_id)
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// The local file of an attachment, downloading it first if needed (and
    /// checking it against its id). Runs off the caller's thread.
    pub async fn fetch_attachment(&self, attachment_id: String) -> Result<String, ChatError> {
        let client = self.client()?;
        let result = background("chat-fetch", move || {
            client
                .fetch_attachment(&attachment_id)
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Replaces the text of one of our sent messages, for everyone
    /// (`MessageUpdated`, here and on other devices).
    pub async fn edit_message(
        &self,
        message_id: String,
        body: String,
    ) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-edit", move || {
            client
                .edit_message(&message_id, &body)
                .map(|m| types::message(m, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Deletes one of our messages for everyone; it becomes a "deleted"
    /// placeholder (`MessageUpdated`). A message that was never sent is
    /// just removed (`MessageRemoved`) and `None` is returned.
    pub async fn delete_message(
        &self,
        message_id: String,
    ) -> Result<Option<ChatMessage>, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-delete", move || {
            client
                .delete_message(&message_id)
                .map(|m| m.map(|m| types::message(m, &user_id)))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Adds (`on`) or removes our `emoji` reaction (`MessageUpdated`).
    pub async fn set_reaction(
        &self,
        message_id: String,
        emoji: String,
        on: bool,
    ) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-react", move || {
            client
                .set_reaction(&message_id, &emoji, on)
                .map(|m| types::message(m, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Saves the composer's unsent text (empty clears it). Kept on this
    /// device across launches; cheap enough to call as the user types.
    pub fn set_draft(&self, conversation_id: String, text: String) -> Result<(), ChatError> {
        self.client()?
            .set_draft(&conversation_id, &text)
            .map_err(Into::into)
    }

    /// The saved draft for a conversation, if any.
    pub fn draft(&self, conversation_id: String) -> Option<String> {
        self.client().ok()?.draft(&conversation_id)
    }

    /// Searches the messages on this device: every word of `query` must
    /// start a word in the message (case and accents ignored). In one
    /// conversation, or in all with `None`. Newest first, at most 100.
    pub async fn search(
        &self,
        query: String,
        conversation_id: Option<String>,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        background("chat-search", move || {
            client
                .search(&query, conversation_id.as_deref())
                .map(|list| {
                    list.into_iter()
                        .map(|m| types::message(m, &user_id))
                        .collect()
                })
                .map_err(ChatError::from)
        })
        .await
    }

    /// How many of our messages are waiting to be sent.
    pub fn outbox_count(&self) -> u32 {
        self.client()
            .map(|c| u32::try_from(c.outbox().len()).unwrap_or(u32::MAX))
            .unwrap_or(0)
    }

    /// Sends a `Failed` (or still queued) message again now, with the same id.
    pub async fn retry_message(&self, message_id: String) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-retry", move || {
            client
                .retry_message(&message_id)
                .map(|m| types::message(m, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Opens the room: it goes live (in the foreground) and is kept read.
    /// Returns at once; `StreamConnected`/`StreamDisconnected` follow.
    pub fn subscribe(&self, conversation_id: String) -> Result<(), ChatError> {
        self.client()?
            .subscribe(&conversation_id)
            .map_err(Into::into)
    }

    /// Closes the room. Its stream ends unless the room is watched.
    pub fn unsubscribe(&self, conversation_id: String) -> Result<(), ChatError> {
        match self.client() {
            Ok(client) => client.unsubscribe(&conversation_id).map_err(Into::into),
            Err(ChatError::NotSignedIn) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Keeps the room live without opening it, so new messages from others
    /// count as unread and raise `Notify`.
    pub fn watch(&self, conversation_id: String) -> Result<(), ChatError> {
        self.client()?.watch(&conversation_id).map_err(Into::into)
    }

    pub fn unwatch(&self, conversation_id: String) -> Result<(), ChatError> {
        match self.client() {
            Ok(client) => client.unwatch(&conversation_id).map_err(Into::into),
            Err(ChatError::NotSignedIn) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Conversations as stored, for a first render before the network.
    pub fn cached_conversations(&self) -> Vec<ChatConversation> {
        self.client_and_user()
            .map(|(c, user_id)| {
                c.cached_conversations()
                    .into_iter()
                    .map(|c| types::conversation(c, &user_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A room's messages as stored, oldest first.
    pub fn cached_messages(&self, conversation_id: String) -> Vec<ChatMessage> {
        self.client_and_user()
            .map(|(c, user_id)| {
                c.cached_messages(&conversation_id)
                    .into_iter()
                    .map(|m| types::message(m, &user_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Room connection threads still running: 0 shortly after every room
    /// is closed, or in the background.
    pub fn live_streams(&self) -> u32 {
        self.client()
            .map(|c| u32::try_from(c.live_pumps()).unwrap_or(u32::MAX))
            .unwrap_or(0)
    }

    /// The last persistence problem, if any. Never fails a command.
    pub fn store_problem(&self) -> Option<String> {
        self.client().ok()?.store_error().map(|e| e.to_string())
    }

    /// The phase last reported with `set_phase`.
    pub fn phase(&self) -> AppPhase {
        self.lifecycle.phase().into()
    }

    /// The host reports a lifecycle change; cheap, safe to repeat. Going to
    /// `Background` stops every live stream; coming back restarts them (they
    /// catch up) and emits `PhaseChanged { resumed: true }`; `Terminated`
    /// is `shutdown()`.
    pub fn set_phase(&self, phase: AppPhase) {
        if phase == AppPhase::Terminated {
            return self.shutdown();
        }
        let Some(change) = self.lifecycle.enter(phase.into()) else {
            return;
        };
        if let Ok(client) = self.client() {
            if change.to == Phase::Background {
                client.pause_streams();
            } else if change.to.is_foreground() {
                // Fails only after shutdown, which `client()` rules out.
                let _ = client.resume_streams();
            }
        }
        self.events.publish(ChatEvent::PhaseChanged {
            phase,
            resumed: change.resumed,
        });
    }

    /// Ends every stream and refuses further commands with `ShutDown`.
    /// Sends in flight finish and are saved. Idempotent.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        self.assistant.shutdown();
        if let Some(session) = self.lock().as_ref() {
            session.client.shutdown();
        }
        if self.lifecycle.enter(Phase::Terminated).is_some() {
            self.events.publish(ChatEvent::PhaseChanged {
                phase: AppPhase::Terminated,
                resumed: false,
            });
        }
        self.events.close();
    }
}

/// The notification category of a new message (`LocalChatNotifier`), with
/// Reply and Mark as Read on it.
pub const MESSAGE_NOTIFICATION: &str = "chat.message";

/// The file whose presence turns the app lock on.
const APP_LOCK_FILE: &str = "app-lock";

/// How the app lock would unlock on this iPhone ("Face ID", "Touch ID",
/// "Passcode"), or nil when it can't lock at all.
#[uniffi::export]
pub fn app_lock_method() -> Option<String> {
    let biometrics = native::biometrics::BiometricsService::installed().ok()?;
    let availability = biometrics.availability();
    availability
        .can_lock()
        .then(|| availability.name().to_string())
}

/// [`MESSAGE_NOTIFICATION`], for the Swift notifier.
#[uniffi::export]
pub fn message_notification_category() -> String {
    MESSAGE_NOTIFICATION.into()
}

impl ChatCore {
    /// Reply and Mark as Read on message notifications, handled without
    /// opening the app. A reply goes through the outbox like any message, so
    /// it is sent later if the phone is offline.
    fn handle_notification_actions(self: &Arc<Self>) {
        let Ok(services) = native::NativeServices::installed() else {
            return;
        };
        let actions = native::NotificationCategory {
            id: MESSAGE_NOTIFICATION.into(),
            actions: vec![
                native::NotificationAction::reply("reply"),
                native::NotificationAction::button("read", "Mark as Read"),
            ],
        };
        if let Err(e) = services.set_notification_categories(vec![actions]) {
            forge_runtime::logging::warn!("notification actions not set: {e}");
        }
        let core = Arc::downgrade(self);
        native::on_notification_response(move |response| {
            let Some(core) = core.upgrade() else { return };
            core.notification_response(response);
        });
    }

    fn notification_response(&self, response: native::NotificationResponse) {
        // The room is the notification's thread (LocalChatNotifier).
        let Some(room) = response.thread_id else {
            return;
        };
        let Ok(client) = self.client() else { return };
        let work = match response.action_id.as_str() {
            "reply" => {
                let Some(text) = response.text.filter(|t| !t.trim().is_empty()) else {
                    return;
                };
                let reply_to = Some(response.notification_id);
                std::thread::Builder::new()
                    .name("chat-notification-reply".into())
                    .spawn(move || {
                        // Never log the reply itself.
                        if let Err(e) = client.send_reply(&room, &text, reply_to.as_deref()) {
                            forge_runtime::logging::warn!("reply from a notification failed: {e}");
                        }
                    })
            }
            "read" => std::thread::Builder::new()
                .name("chat-notification-read".into())
                .spawn(move || {
                    if let Err(e) = client.mark_read(&room) {
                        forge_runtime::logging::warn!("mark read from a notification failed: {e}");
                    }
                }),
            _ => return,
        };
        if let Err(e) = work {
            forge_runtime::logging::warn!("notification action not started: {e}");
        }
    }

    /// Opens with any credential store (tests pass fakes).
    pub fn open(
        storage_directory: PathBuf,
        credentials: Arc<dyn Credentials>,
    ) -> Result<Self, ChatError> {
        forge_runtime::logging::init();
        if storage_directory.as_os_str().is_empty() {
            return Err(ChatError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        std::fs::create_dir_all(&storage_directory).map_err(|err| ChatError::Storage {
            reason: format!("create {}: {err}", storage_directory.display()),
        })?;
        let assistant =
            AssistantCore::open(&storage_directory.join("assistant.db")).map_err(|err| {
                ChatError::Storage {
                    reason: format!("open the assistant: {err}"),
                }
            })?;
        let core = Self {
            assistant,
            storage_directory,
            credentials,
            session: Mutex::new(None),
            events: EventHub::with_capacity(EVENT_QUEUE_CAPACITY),
            lifecycle: Lifecycle::new(),
            shut_down: AtomicBool::new(false),
        };
        if let Some(stored) = core.credentials.load()? {
            let session = core.open_session(&stored)?;
            *core.lock() = Some(session);
        }
        Ok(core)
    }

    fn lock(&self) -> MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn ensure_running(&self) -> Result<(), ChatError> {
        if self.shut_down.load(Ordering::SeqCst) {
            Err(ChatError::ShutDown)
        } else {
            Ok(())
        }
    }

    fn client(&self) -> Result<ChatClient, ChatError> {
        self.client_and_user().map(|(client, _)| client)
    }

    fn client_and_user(&self) -> Result<(ChatClient, String), ChatError> {
        self.ensure_running()?;
        self.lock()
            .as_ref()
            .map(|s| (s.client.clone(), s.account.user_id.clone()))
            .ok_or(ChatError::NotSignedIn)
    }

    /// Where an account's chats are kept.
    fn account_directory(&self, user_id: &str) -> PathBuf {
        let safe: String = user_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.storage_directory.join("accounts").join(safe)
    }

    fn open_session(&self, stored: &StoredAccount) -> Result<Session, ChatError> {
        let directory = self.account_directory(&stored.user_id);
        std::fs::create_dir_all(&directory).map_err(|err| ChatError::Storage {
            reason: format!("create {}: {err}", directory.display()),
        })?;
        let repo = server_repo(&stored.server_url).with_token(&stored.token);
        let config = ChatConfig {
            user: stored.display_name.clone(),
            user_id: stored.user_id.clone(),
            ..ChatConfig::default()
        };
        let hub = self.events.clone();
        let user_id = stored.user_id.clone();
        let client = ChatClient::open(
            Arc::new(repo),
            ChatStore::in_directory(directory),
            config,
            move |event: chat_core::ChatEvent| hub.publish(types::event(event, &user_id)),
        )?;
        if self.lifecycle.phase() == Phase::Background {
            client.pause_streams();
        }
        Ok(Session {
            account: stored.public(),
            client,
        })
    }

    fn sign_out_locked(
        &self,
        session: &mut Option<Session>,
        announce: bool,
    ) -> Result<(), ChatError> {
        if let Some(old) = session.take() {
            old.client.shutdown();
            remove_directory(&self.account_directory(&old.account.user_id));
        }
        self.credentials.delete()?;
        if announce {
            self.events
                .publish(ChatEvent::AccountChanged { account: None });
        }
        Ok(())
    }

    /// Announces a rejected token, then hands the error back.
    fn rejected(&self, error: ChatError) -> ChatError {
        if let ChatError::Unauthorized { reason } = &error {
            self.events.publish(ChatEvent::AccountRejected {
                reason: reason.clone(),
            });
        }
        error
    }
}

/// The chat server's API, through URLSession once the host has installed
/// the `http` service (on iOS), and through ureq before that and in tests.
fn server_repo(server_url: impl Into<String>) -> HttpRepo {
    static CLIENT: std::sync::OnceLock<Arc<native::http::PreferInstalled>> =
        std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        Arc::new(native::http::HttpService::prefer_installed(
            Arc::new(forge_net::UreqClient::with_user_agent(concat!(
                "forge-chat-example/",
                env!("CARGO_PKG_VERSION")
            ))),
            |client| client,
        ))
    });
    HttpRepo::new(
        Arc::clone(client) as Arc<dyn forge_net::HttpClient>,
        server_url,
    )
}

/// Checks the server URL the user typed: http(s), no trailing slash.
fn normalise_server_url(url: &str) -> Result<String, ChatError> {
    let url = url.trim().trim_end_matches('/');
    forge_net::Request::get(url)
        .validate()
        .map_err(|_| ChatError::InvalidInput {
            reason: format!("server URL must start with http:// or https://: {url:?}"),
        })?;
    Ok(url.to_string())
}

fn remove_directory(path: &Path) {
    if let Err(err) = std::fs::remove_dir_all(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            forge_runtime::logging::warn!(target: "chat", "could not remove {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
mod tests;
