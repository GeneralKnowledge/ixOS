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

pub mod credentials;
mod types;

pub use credentials::{Credentials, KeychainCredentials, MemoryCredentials, StoredAccount};
pub use types::*;

uniffi::setup_scaffolding!("forge_ffi");

// The lifecycle phase the host reports and the event stream object
// (`ChatEvents::subscribe(&hub)`). See `forge_runtime::app_shell!`.
forge_runtime::app_shell! {
    phase: AppPhase,
    events: ChatEvents(ChatEvent::EventsDropped),
}

// Native services the host implements (Keychain, directories, notifications,
// permissions, deep links), as `crate::native`. The app installs them with
// `ForgeServices.install()`; see docs/native-services.md.
forge_services::native_services!();

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
        Self::open(PathBuf::from(storage_directory), Arc::new(credentials)).map(Arc::new)
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
                HttpRepo::with_ureq(server_url)
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
            HttpRepo::with_ureq(&stored.server_url)
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

    /// A new event stream. Each consumer takes its own and sees every event
    /// from now on.
    pub fn events(&self) -> Result<Arc<ChatEvents>, ChatError> {
        ChatEvents::subscribe(&self.events).ok_or(ChatError::ShutDown)
    }

    /// Lists conversations from the server, prefetching new messages for
    /// rooms that are not open (they count as unread).
    pub async fn list_conversations(&self) -> Result<Vec<ChatConversation>, ChatError> {
        let client = self.client()?;
        let result = background("chat-list", move || {
            client
                .list_conversations()
                .map(|list| list.into_iter().map(Into::into).collect())
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
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
    /// `Sent` or `Failed` (`MessageStatusChanged`). Resolves with the final
    /// message, or the error that failed it.
    pub async fn send_message(
        &self,
        conversation_id: String,
        body: String,
    ) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let result = background("chat-send", move || {
            client
                .send_message(&conversation_id, &body)
                .map(|m| types::message(m, &user_id))
                .map_err(ChatError::from)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Sends a `Failed` message again with the same id.
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
        self.client()
            .map(|c| {
                c.cached_conversations()
                    .into_iter()
                    .map(Into::into)
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

impl ChatCore {
    /// Opens with any credential store (tests pass fakes).
    pub fn open(
        storage_directory: PathBuf,
        credentials: Arc<dyn Credentials>,
    ) -> Result<Self, ChatError> {
        if storage_directory.as_os_str().is_empty() {
            return Err(ChatError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        std::fs::create_dir_all(&storage_directory).map_err(|err| ChatError::Storage {
            reason: format!("create {}: {err}", storage_directory.display()),
        })?;
        let core = Self {
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
        let repo = HttpRepo::with_ureq(&stored.server_url).with_token(&stored.token);
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
            eprintln!("[chat] could not remove {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
mod tests;
