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
//! - **Lifecycle.** The core runs on forge-app's runtime
//!   (`export_runtime!`), which ForgeKit's ForgeHost drives: one core per
//!   data folder, the phases the host reports, shutdown. In the background
//!   every live stream stops (iOS suspends the app within seconds anyway),
//!   and on resume they restart and catch up. The chat's own events are a
//!   second stream (`chat_events`).
//! - **Assistant.** A private assistant that runs on the iPhone
//!   ([`assistant`], `ChatCore::assistant`). It has its own store and works
//!   signed in or out.
//!
//! Network commands are `async` and run on worker threads
//! ([`forge_runtime::background`]). They are not cancelled by the host: a
//! send that has started finishes, and the event stream reports how it
//! ended (see `examples/relay/docs/chat-demo.md`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use chat_core::{ChatClient, ChatConfig, ChatStore, HttpRepo};

use crate::native;
use forge_app::{DataDirectory, Runtime};
use forge_net::AbortRegistration;
use forge_runtime::{background, EventHub, Phase, Transition};

pub mod alerts;
pub mod assistant;
pub mod credentials;
mod types;

pub use assistant::AssistantCore;
pub use credentials::{Credentials, KeychainCredentials, MemoryCredentials, StoredAccount};
pub use forge_app::AppPhase;
pub use types::*;

// The chat's own event stream object (`ChatEvents::subscribe(&hub)`). The
// lifecycle phase is forge-app's `AppPhase`. See `forge_runtime::app_shell!`.
forge_runtime::app_shell! {
    events: ChatEvents(ChatEvent::EventsDropped),
}

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
    /// The runtime: the data folder's lock, phases, shutdown.
    rt: Runtime<()>,
    /// Ends the chat when the runtime shuts down (kept so the hook stays).
    _on_shutdown: Mutex<Option<AbortRegistration>>,
    assistant: Arc<AssistantCore>,
    /// The notifier for message notifications; none in tests that don't
    /// ask for one, and in `swift test` (no app bundle).
    services: Option<native::NativeServices>,
    /// Conversations with trusted devices (`crate::device_chat`), one
    /// client each, beside the server account's.
    devices: Mutex<DeviceChats>,
    /// Held by the lifecycle thread while it applies a phase change, and
    /// set to true by shutdown (`end`). Once shutdown returns, that thread
    /// never takes the core again, so dropping the owner's last reference
    /// frees the core and its data folder's lock at once.
    lifecycle_gate: Arc<Mutex<bool>>,
}

/// The trusted devices' conversations: Relay's core, and a chat-core
/// client per device, opened on first use. Each has its own client so a
/// device that is away holds up only its own outbox.
#[derive(Default)]
struct DeviceChats {
    relay: Option<Weak<crate::RelayCore>>,
    clients: std::collections::HashMap<String, ChatClient>,
}

/// The signed-in account and its chat engine.
struct Session {
    account: ChatAccount,
    client: ChatClient,
    /// Message notifications, when the app has a notifier.
    alerts: Option<Arc<alerts::Alerts>>,
}

/// The background refresh (forge.toml `[ios] background_tasks`): catches up
/// every room and announces what arrived, when iOS gives the app time.
pub const BACKGROUND_REFRESH: &str = "org.forgemobile.chat.refresh";

/// How often to ask iOS for a refresh. iOS decides when (and whether) it
/// runs, from how the person uses the app; it is a floor, not a schedule.
pub const BACKGROUND_REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[uniffi::export]
impl ChatCore {
    /// Opens the app with its account in the iOS Keychain. Needs
    /// `ForgeServices.install()` first; without it this fails with
    /// `UnsupportedOnHost`. `storage_directory` (created if missing) holds
    /// each account's chats.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, ChatError> {
        let credentials = KeychainCredentials::installed()?;
        let services = native::NativeServices::installed().ok();
        let core = Self::open_with_services(
            PathBuf::from(storage_directory),
            Arc::new(credentials),
            services,
        )?;
        core.handle_notification_actions();
        core.ask_for_notifications();
        core.register_background_refresh();
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

    /// Adds a conversation with each device Relay trusts to the list,
    /// beside the server's, working with or without an account. Messages
    /// to a device that is away wait in its outbox and go when Relay
    /// reaches it.
    pub fn use_relay(self: Arc<Self>, relay: Arc<crate::RelayCore>) {
        let me = Arc::downgrade(&self);
        relay.set_chat_waker(Some(Arc::new(move |endpoint_id: &str| {
            if let Some(core) = me.upgrade() {
                let client = core.devices_lock().clients.get(endpoint_id).cloned();
                if let Some(client) = client {
                    client.flush_outbox();
                }
            }
        })));
        self.devices_lock().relay = Some(Arc::downgrade(&relay));
    }

    /// The on-device assistant. Works without an account, and keeps its
    /// conversations when the account signs out.
    pub fn assistant(&self) -> Arc<AssistantCore> {
        Arc::clone(&self.assistant)
    }

    /// A new event stream. Each consumer takes its own and sees every event
    /// from now on.
    pub fn chat_events(&self) -> Result<Arc<ChatEvents>, ChatError> {
        ChatEvents::subscribe(&self.events).ok_or(ChatError::ShutDown)
    }

    /// Lists conversations from the server, prefetching new messages for
    /// rooms that are not open (they count as unread).
    /// With Relay (`use_relay`), each trusted device's conversation is
    /// listed too, and the list works signed out.
    pub async fn list_conversations(&self) -> Result<Vec<ChatConversation>, ChatError> {
        let server = match self.client_and_user() {
            Ok(found) => Some(found),
            Err(ChatError::NotSignedIn) if self.has_relay() => None,
            Err(err) => return Err(err),
        };
        let devices = self.device_clients();
        let alerts = self.alerts();
        let result = background("chat-list", move || {
            let mut out = Vec::new();
            if let Some((client, user_id)) = server {
                let list = client.list_conversations().map_err(ChatError::from)?;
                if let Some(alerts) = &alerts {
                    watch_all(&client, alerts);
                }
                out.extend(list.into_iter().map(|c| types::conversation(c, &user_id)));
            }
            for (client, user_id) in devices {
                match client.list_conversations() {
                    Ok(list) => {
                        out.extend(list.into_iter().map(|c| types::conversation(c, &user_id)))
                    }
                    Err(e) => forge_runtime::logging::warn!("a device conversation: {e}"),
                }
            }
            Ok(out)
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Creates a conversation on the server. It is added to the list
    /// (`ConversationUpdated`) and returned, ready to open.
    pub async fn create_conversation(&self, title: String) -> Result<ChatConversation, ChatError> {
        let (client, user_id) = self.client_and_user()?;
        let alerts = self.alerts();
        let result = background("chat-create", move || {
            let created = client
                .create_conversation(&title)
                .map_err(ChatError::from)?;
            if alerts.is_some() {
                // Best effort: the next list watches it anyway.
                let _ = client.watch(&created.id);
            }
            Ok(types::conversation(created, &user_id))
        })
        .await;
        result.map_err(|e| self.rejected(e))
    }

    /// Marks everything in the room read, here at once and then on the
    /// server (other devices and other people see it). Clears "marked
    /// unread". An open room is kept read without this.
    pub async fn mark_read(&self, conversation_id: String) -> Result<(), ChatError> {
        let client = self.route(&conversation_id)?.0;
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
        self.route(&conversation_id)?
            .0
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
        self.route(&conversation_id)?
            .0
            .set_muted(&conversation_id, muted)
            .map_err(Into::into)
    }

    /// Fetches the room's messages newer than what is stored, merges them
    /// and returns the room's full stored list, oldest first.
    pub async fn sync_messages(
        &self,
        conversation_id: String,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let (client, user_id) = self.route(&conversation_id)?;
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
        let (client, user_id) = self.route(&conversation_id)?;
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
        let (client, user_id) = self.route(&conversation_id)?;
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
    /// without an extension. An attachment in a device conversation is
    /// where Relay keeps it: the saved file or folder (a folder opens in
    /// Files).
    pub fn attachment_file(&self, attachment_id: String) -> Option<String> {
        let server = self.client().ok().and_then(|client| {
            client
                .attachment_file(&attachment_id)
                .map(|p| p.to_string_lossy().into_owned())
        });
        server.or_else(|| self.relay()?.device_attachment(&attachment_id))
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
        let (client, user_id) = self.route_message(&message_id)?;
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
        let (client, user_id) = self.route_message(&message_id)?;
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
        let (client, user_id) = self.route_message(&message_id)?;
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
        self.route(&conversation_id)?
            .0
            .set_draft(&conversation_id, &text)
            .map_err(Into::into)
    }

    /// The saved draft for a conversation, if any.
    pub fn draft(&self, conversation_id: String) -> Option<String> {
        self.route(&conversation_id).ok()?.0.draft(&conversation_id)
    }

    /// Searches the messages on this device: every word of `query` must
    /// start a word in the message (case and accents ignored). In one
    /// conversation, or in all with `None`. Newest first, at most 100.
    pub async fn search(
        &self,
        query: String,
        conversation_id: Option<String>,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        let clients = match &conversation_id {
            Some(id) => vec![self.route(id)?],
            None => self.all_clients()?,
        };
        background("chat-search", move || {
            let mut found = Vec::new();
            for (client, user_id) in clients {
                let list = client
                    .search(&query, conversation_id.as_deref())
                    .map_err(ChatError::from)?;
                found.extend(list.into_iter().map(|m| types::message(m, &user_id)));
            }
            found.sort_by_key(|m: &ChatMessage| std::cmp::Reverse(m.sent_at));
            found.truncate(100);
            Ok(found)
        })
        .await
    }

    /// How many of our messages are waiting to be sent.
    pub fn outbox_count(&self) -> u32 {
        let waiting: usize = self
            .all_clients()
            .unwrap_or_default()
            .iter()
            .map(|(c, _)| c.outbox().len())
            .sum();
        u32::try_from(waiting).unwrap_or(u32::MAX)
    }

    /// Sends a `Failed` (or still queued) message again now, with the same id.
    pub async fn retry_message(&self, message_id: String) -> Result<ChatMessage, ChatError> {
        let (client, user_id) = self.route_message(&message_id)?;
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
        self.route(&conversation_id)?
            .0
            .subscribe(&conversation_id)
            .map_err(Into::into)
    }

    /// Closes the room. Its stream ends unless the room is watched.
    pub fn unsubscribe(&self, conversation_id: String) -> Result<(), ChatError> {
        match self.route(&conversation_id).map(|(c, _)| c) {
            Ok(client) => client.unsubscribe(&conversation_id).map_err(Into::into),
            Err(ChatError::NotSignedIn) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Keeps the room live without opening it, so new messages from others
    /// count as unread and raise `Notify`.
    pub fn watch(&self, conversation_id: String) -> Result<(), ChatError> {
        self.route(&conversation_id)?
            .0
            .watch(&conversation_id)
            .map_err(Into::into)
    }

    pub fn unwatch(&self, conversation_id: String) -> Result<(), ChatError> {
        match self.route(&conversation_id).map(|(c, _)| c) {
            Ok(client) => client.unwatch(&conversation_id).map_err(Into::into),
            Err(ChatError::NotSignedIn) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Conversations as stored, for a first render before the network.
    pub fn cached_conversations(&self) -> Vec<ChatConversation> {
        self.all_clients()
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(c, user_id)| {
                c.cached_conversations()
                    .into_iter()
                    .map(move |c| types::conversation(c, &user_id))
            })
            .collect()
    }

    /// A room's messages as stored, oldest first.
    pub fn cached_messages(&self, conversation_id: String) -> Vec<ChatMessage> {
        self.route(&conversation_id)
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
        let live: usize = self
            .all_clients()
            .unwrap_or_default()
            .iter()
            .map(|(c, _)| c.live_pumps())
            .sum();
        u32::try_from(live).unwrap_or(u32::MAX)
    }

    /// The last persistence problem, if any. Never fails a command.
    pub fn store_problem(&self) -> Option<String> {
        self.client().ok()?.store_error().map(|e| e.to_string())
    }
}

forge_app::export_runtime!(ChatCore, rt);

/// The notification category of a new message ([`alerts`]), with Reply and
/// Mark as Read on it.
pub const MESSAGE_NOTIFICATION: &str = "chat.message";

/// The deep link on a message's notification
/// (`forgechat://conversation/<room>?message=<id>`).
#[uniffi::export]
pub fn chat_message_link(conversation_id: String, message_id: String) -> String {
    alerts::message_link(&conversation_id, &message_id)
}

/// The conversation in Chats of the trusted device `endpoint_id` (its
/// device screen's Message button opens it).
#[uniffi::export]
pub fn device_conversation_id(endpoint_id: String) -> String {
    crate::device_chat::device_conversation(&endpoint_id)
}

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

    /// Asks once for notifications (iOS remembers the answer), off the
    /// caller's thread.
    fn ask_for_notifications(&self) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("chat-ask-notifications".into())
            .spawn(move || {
                forge_runtime::block_on(
                    services.request_permission(native::NativePermission::Notifications),
                );
            });
    }

    /// Asks iOS for [`BACKGROUND_REFRESH`] windows. Does nothing where the
    /// host has no background tasks (tests, Linux).
    fn register_background_refresh(self: &Arc<Self>) {
        if let Ok(service) = native::background::BackgroundService::installed() {
            self.register_background_refresh_with(&service);
        }
    }

    /// [`register_background_refresh`](Self::register_background_refresh)
    /// with a given service (tests pass a fake scheduler).
    pub fn register_background_refresh_with(
        self: &Arc<Self>,
        service: &native::background::BackgroundService,
    ) {
        let core = Arc::downgrade(self);
        let registered = service.register_refresh(
            BACKGROUND_REFRESH,
            BACKGROUND_REFRESH_EVERY.as_secs_f64(),
            move |run| {
                let core = core.clone();
                async move {
                    let Some(core) = core.upgrade() else {
                        return false;
                    };
                    // Each run has its own thread, so blocking is fine here.
                    core.catch_up_in_background(&|| run.is_cancelled()).is_ok()
                }
            },
        );
        if let Err(e) = registered {
            forge_runtime::logging::warn!("background refresh not registered: {e}");
        }
    }

    /// One background refresh: lists the rooms, fetches what is new in each,
    /// sends what waits in the outbox, and announces new messages from
    /// others. `cancelled` is checked between rooms (iOS ending the window).
    /// Returns how many notifications it posted.
    pub fn catch_up_in_background(&self, cancelled: &dyn Fn() -> bool) -> Result<u32, ChatError> {
        let client = match self.client() {
            Ok(client) => client,
            // Signed out: nothing to fetch, and nothing went wrong.
            Err(ChatError::NotSignedIn) => return Ok(0),
            Err(e) => return Err(e),
        };
        let started = std::time::Instant::now();
        let rooms = client
            .list_conversations()
            .map_err(|e| self.rejected(e.into()))?;
        for room in &rooms {
            if cancelled() {
                forge_runtime::logging::info!("background refresh cut short by iOS");
                return Err(ChatError::Cancelled);
            }
            if let Err(e) = client.sync_messages(&room.id) {
                forge_runtime::logging::warn!("background refresh: a room failed: {e}");
            }
        }
        client.flush_outbox();
        let announced = self
            .alerts()
            .map_or(0, |alerts| alerts.announce_unread(&client));
        forge_runtime::logging::info!(
            "background refresh: {} rooms, {} announced, {} ms",
            rooms.len(),
            announced,
            started.elapsed().as_millis()
        );
        Ok(u32::try_from(announced).unwrap_or(u32::MAX))
    }

    fn notification_response(&self, response: native::NotificationResponse) {
        // The room is the notification's thread (alerts::notification).
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

    /// Opens with any credential store (tests pass fakes), without
    /// notifications.
    pub fn open(
        storage_directory: PathBuf,
        credentials: Arc<dyn Credentials>,
    ) -> Result<Arc<Self>, ChatError> {
        Self::open_with_services(storage_directory, credentials, None)
    }

    /// Opens with any credential store and, with `services`, message
    /// notifications through its notifier: every room is then kept live
    /// while the app is on screen, so its news can be announced.
    pub fn open_with_services(
        storage_directory: PathBuf,
        credentials: Arc<dyn Credentials>,
        services: Option<native::NativeServices>,
    ) -> Result<Arc<Self>, ChatError> {
        forge_runtime::logging::init();
        if storage_directory.as_os_str().is_empty() {
            return Err(ChatError::InvalidInput {
                reason: "storage directory must not be empty".into(),
            });
        }
        // One core per data folder: a second is refused with `InUse`.
        let directory = DataDirectory::open(&storage_directory)?;
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
            rt: Runtime::with_directory("chat", directory, ()),
            _on_shutdown: Mutex::new(None),
            services,
            devices: Mutex::new(DeviceChats::default()),
            lifecycle_gate: Arc::new(Mutex::new(false)),
        };
        if let Some(stored) = core.credentials.load()? {
            let session = core.open_session(&stored)?;
            *core.lock() = Some(session);
        }
        let core = Arc::new(core);
        core.follow_lifecycle();
        Ok(core)
    }

    /// Applies the phases the host reports (through the runtime) to the
    /// chat: streams stop in the background and catch up on return, and
    /// shutdown ends the chat. Shutdown runs at once, on the caller's
    /// thread (it is quick); the other changes on a thread of their own.
    fn follow_lifecycle(self: &Arc<Self>) {
        let lifecycle = self.rt.lifecycle();
        let core = Arc::downgrade(self);
        let registration = lifecycle.lifetime().on_cancel(move || {
            if let Some(core) = core.upgrade() {
                core.end();
            }
        });
        *self._on_shutdown.lock().unwrap_or_else(|p| p.into_inner()) = Some(registration);
        let Some(changes) = lifecycle.subscribe() else {
            return;
        };
        let core: Weak<Self> = Arc::downgrade(self);
        let gate = Arc::clone(&self.lifecycle_gate);
        std::thread::Builder::new()
            .name("chat-lifecycle".into())
            .spawn(move || {
                while let Some(delivery) = forge_runtime::block_on(changes.next()) {
                    let forge_runtime::Delivery::Event(change) = delivery else {
                        continue;
                    };
                    // Taking the core here keeps it, and its data folder's
                    // lock, alive for as long as the change takes. Without
                    // the gate that could outlast shutdown and the owner
                    // dropping the core, so a core reopened at once on the
                    // same folder was refused as `InUse` (seen in CI).
                    let shut_down = gate.lock().unwrap_or_else(|p| p.into_inner());
                    if *shut_down || change.to == Phase::Terminated {
                        return;
                    }
                    let Some(core) = core.upgrade() else { return };
                    core.apply(change);
                    drop(core);
                    drop(shut_down);
                }
            })
            .expect("a thread for the lifecycle");
    }

    /// One phase change, as `set_phase` used to apply it.
    fn apply(&self, change: Transition) {
        if change.to == Phase::Terminated {
            return;
        }
        for (client, _) in self.all_clients().unwrap_or_default() {
            if change.to == Phase::Background {
                client.pause_streams();
            } else if change.to.is_foreground() {
                // Fails only after shutdown, which `all_clients()` rules out.
                let _ = client.resume_streams();
            }
        }
        if change.to.is_foreground() {
            // What arrived until now is on screen or counted as unread.
            if let Some(alerts) = self.alerts() {
                alerts.seen_until(forge_services::notify::now_ms());
            }
        }
        self.events.publish(ChatEvent::PhaseChanged {
            phase: change.to.into(),
            resumed: change.resumed,
        });
    }

    /// Ends every stream and the chat's event stream. Sends in flight
    /// finish and are saved.
    fn end(&self) {
        // Waits for a phase change the lifecycle thread is applying, and
        // stops it taking the core again (`lifecycle_gate`).
        *self
            .lifecycle_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = true;
        self.assistant.shutdown();
        if let Some(session) = self.lock().as_ref() {
            session.client.shutdown();
        }
        for client in self.devices_lock().clients.values() {
            client.shutdown();
        }
        self.events.publish(ChatEvent::PhaseChanged {
            phase: AppPhase::Terminated,
            resumed: false,
        });
        self.events.close();
    }

    fn lock(&self) -> MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn ensure_running(&self) -> Result<(), ChatError> {
        if self.rt.is_shut_down() {
            Err(ChatError::ShutDown)
        } else {
            Ok(())
        }
    }

    fn alerts(&self) -> Option<Arc<alerts::Alerts>> {
        self.lock().as_ref().and_then(|s| s.alerts.clone())
    }

    fn devices_lock(&self) -> MutexGuard<'_, DeviceChats> {
        self.devices.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn relay(&self) -> Option<Arc<crate::RelayCore>> {
        self.devices_lock().relay.as_ref().and_then(Weak::upgrade)
    }

    fn has_relay(&self) -> bool {
        self.relay().is_some()
    }

    /// The client of the conversation: a trusted device's, or the server
    /// account's.
    fn route(&self, conversation_id: &str) -> Result<(ChatClient, String), ChatError> {
        match crate::device_chat::conversation_device(conversation_id) {
            Some(endpoint_id) => self.device_client(endpoint_id),
            None => self.client_and_user(),
        }
    }

    /// The client that holds the message `message_id`.
    fn route_message(&self, message_id: &str) -> Result<(ChatClient, String), ChatError> {
        for (client, user_id) in self.device_clients() {
            let holds = client.cached_conversations().iter().any(|c| {
                client
                    .cached_messages(&c.id)
                    .iter()
                    .any(|m| m.id == message_id)
            });
            if holds {
                return Ok((client, user_id));
            }
        }
        self.client_and_user()
    }

    /// The server account's client, if signed in, and every trusted
    /// device's.
    fn all_clients(&self) -> Result<Vec<(ChatClient, String)>, ChatError> {
        let mut all = match self.client_and_user() {
            Ok(found) => vec![found],
            Err(ChatError::NotSignedIn) => Vec::new(),
            Err(err) => return Err(err),
        };
        all.extend(self.device_clients());
        Ok(all)
    }

    /// The clients of the devices Relay trusts now, opening any that
    /// aren't open; a device no longer trusted has its client closed.
    fn device_clients(&self) -> Vec<(ChatClient, String)> {
        let Some(relay) = self.relay() else {
            return Vec::new();
        };
        let trusted: Vec<String> = relay
            .devices()
            .into_iter()
            .filter(|d| d.trusted)
            .map(|d| d.endpoint_id)
            .collect();
        self.devices_lock().clients.retain(|id, client| {
            let keep = trusted.contains(id);
            if !keep {
                client.shutdown();
            }
            keep
        });
        trusted
            .iter()
            .filter_map(|id| match self.device_client(id) {
                Ok(found) => Some(found),
                Err(e) => {
                    forge_runtime::logging::warn!("a device conversation didn't open: {e}");
                    None
                }
            })
            .collect()
    }

    /// The client of `endpoint_id`'s conversation, opened on first use.
    fn device_client(&self, endpoint_id: &str) -> Result<(ChatClient, String), ChatError> {
        self.ensure_running()?;
        let relay = self.relay().ok_or(ChatError::NotSignedIn)?;
        let identity = relay.identity();
        let user_id = format!("relay:{}", identity.device_id);
        if let Some(client) = self.devices_lock().clients.get(endpoint_id) {
            return Ok((client.clone(), user_id));
        }
        let trusted = relay
            .devices()
            .iter()
            .any(|d| d.trusted && d.endpoint_id == endpoint_id);
        let conversation_id = crate::device_chat::device_conversation(endpoint_id);
        if !trusted {
            return Err(ChatError::NotFound { conversation_id });
        }
        let repo = relay
            .peer_repo(endpoint_id)
            .map_err(|e| ChatError::Storage {
                reason: e.to_string(),
            })?;
        let directory = self.storage_directory.join("devices").join(endpoint_id);
        std::fs::create_dir_all(&directory).map_err(|err| ChatError::Storage {
            reason: format!("create {}: {err}", directory.display()),
        })?;
        let config = ChatConfig {
            user: identity.name,
            user_id: user_id.clone(),
            ..ChatConfig::default()
        };
        let hub = self.events.clone();
        let events_user = user_id.clone();
        let client = ChatClient::open(
            Arc::new(repo),
            ChatStore::in_directory(directory),
            config,
            move |event: chat_core::ChatEvent| hub.publish(types::event(event, &events_user)),
        )?;
        if self.rt.lifecycle().phase() == Phase::Background {
            client.pause_streams();
        }
        // Live while Relay is open, so what arrives counts as unread.
        if let Err(e) = client.watch(&conversation_id) {
            forge_runtime::logging::warn!("a device conversation isn't live: {e}");
        }
        let mut devices = self.devices_lock();
        if let Some(opened) = devices.clients.get(endpoint_id) {
            // Opened by another command meanwhile: keep that one.
            client.shutdown();
            return Ok((opened.clone(), user_id));
        }
        devices
            .clients
            .insert(endpoint_id.to_string(), client.clone());
        Ok((client, user_id))
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
        let alerts = self
            .services
            .clone()
            .map(|services| Arc::new(alerts::Alerts::open(services, directory.clone())));
        let hub = self.events.clone();
        let user_id = stored.user_id.clone();
        let announcer = alerts.clone();
        let client = ChatClient::open(
            Arc::new(repo),
            ChatStore::in_directory(directory),
            config,
            move |event: chat_core::ChatEvent| {
                if let Some(alerts) = &announcer {
                    match &event {
                        chat_core::ChatEvent::ConversationUpdated(room) => {
                            alerts.remember_title(&room.id, &room.title)
                        }
                        chat_core::ChatEvent::Notify(message) => {
                            alerts.announce(message);
                        }
                        _ => {}
                    }
                }
                hub.publish(types::event(event, &user_id))
            },
        )?;
        if self.rt.lifecycle().phase() == Phase::Background {
            client.pause_streams();
        }
        if let Some(alerts) = &alerts {
            watch_all(&client, alerts);
        }
        Ok(Session {
            account: stored.public(),
            client,
            alerts,
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

/// Keeps every stored room live, so news in rooms that aren't open can be
/// announced, and gives the notifications the rooms' titles. While the app
/// is in the background this only records them.
fn watch_all(client: &ChatClient, alerts: &alerts::Alerts) {
    for room in client.cached_conversations() {
        alerts.remember_title(&room.id, &room.title);
        if let Err(e) = client.watch(&room.id) {
            forge_runtime::logging::warn!("room not watched: {e}");
        }
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
