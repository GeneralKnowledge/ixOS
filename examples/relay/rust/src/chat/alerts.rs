//! Message notifications (M5): the Rust core decides what to tell the
//! person and posts it through `forge-services`' notifier, so the same rules
//! hold in the foreground, after a background refresh, in the standalone app
//! and inside Device Lab.
//!
//! - A new message from someone else, in a room that is not on screen and
//!   not muted, gets one notification: the room as title, the author as
//!   subtitle, the text (or "Photo", "Voice message", the file name), grouped
//!   by room, with Reply and Mark as Read ([`crate::chat::MESSAGE_NOTIFICATION`])
//!   and a deep link that opens the room at that message.
//! - Nothing older than the last time the app was on screen is announced,
//!   and nothing twice: the floor and the ids already announced are kept in
//!   `alerts.json` beside the account's chats, so a refresh in a new process
//!   doesn't repeat what an earlier one showed.
//! - Message text is never logged.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use chat_core::{ChatClient, Message};
use serde::{Deserialize, Serialize};

use crate::native;

/// Scheme of the links on message notifications (forge.toml `url_schemes`).
pub const LINK_SCHEME: &str = "forgechat";

/// Announced ids kept per account, newest first, before the oldest go.
const REMEMBERED: usize = 500;

/// Most notifications one background refresh posts; the rest show as
/// unread counts when the app opens.
pub const MAX_PER_REFRESH: usize = 10;

#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    /// Messages sent at or before this (Unix ms) are never announced: the
    /// person had the app open after them.
    floor: i64,
    /// Announced message ids and their `sent_at`, above the floor.
    announced: BTreeMap<String, i64>,
}

/// Posts message notifications for one account.
pub struct Alerts {
    services: native::NativeServices,
    path: PathBuf,
    ledger: Mutex<Ledger>,
    /// Room titles, kept current from the client's events.
    titles: Mutex<HashMap<String, String>>,
}

impl Alerts {
    /// For the account whose chats are in `directory`. The first time, the
    /// floor is now, so an account's history is never announced.
    pub fn open(services: native::NativeServices, directory: PathBuf) -> Self {
        let path = directory.join("alerts.json");
        let ledger = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| Ledger {
                floor: forge_services::notify::now_ms(),
                announced: BTreeMap::new(),
            });
        let alerts = Self {
            services,
            path,
            ledger: Mutex::new(ledger),
            titles: Mutex::new(HashMap::new()),
        };
        alerts.save(&alerts.lock());
        alerts
    }

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save(&self, ledger: &Ledger) {
        let written = serde_json::to_vec(ledger)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                forge_storage::write_atomic(&self.path, &bytes).map_err(|e| e.to_string())
            });
        if let Err(e) = written {
            forge_runtime::logging::warn!("notification record not saved: {e}");
        }
    }

    /// The person has the app on screen: everything sent until now has been
    /// seen (or is visible as unread), so it is never announced later.
    pub fn seen_until(&self, now_ms: i64) {
        let mut ledger = self.lock();
        if now_ms <= ledger.floor {
            return;
        }
        ledger.floor = now_ms;
        ledger.announced.retain(|_, sent_at| *sent_at > now_ms);
        self.save(&ledger);
    }

    /// Claims `message` for a notification: false when it is at or below
    /// the floor or was announced before.
    fn claim(&self, message: &Message) -> bool {
        let mut ledger = self.lock();
        if message.sent_at <= ledger.floor || ledger.announced.contains_key(&message.id) {
            return false;
        }
        ledger.announced.insert(message.id.clone(), message.sent_at);
        if ledger.announced.len() > REMEMBERED {
            // Forget the oldest; raising the floor to them keeps them quiet.
            let mut by_age: Vec<(i64, String)> = ledger
                .announced
                .iter()
                .map(|(id, sent)| (*sent, id.clone()))
                .collect();
            by_age.sort();
            let drop = by_age.len() - REMEMBERED;
            for (sent, id) in by_age.into_iter().take(drop) {
                ledger.announced.remove(&id);
                ledger.floor = ledger.floor.max(sent);
            }
        }
        self.save(&ledger);
        true
    }

    /// Remembers a room's title for its notifications.
    pub fn remember_title(&self, conversation_id: &str, title: &str) {
        self.titles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(conversation_id.into(), title.into());
    }

    fn title(&self, conversation_id: &str) -> String {
        self.titles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(conversation_id)
            .cloned()
            .unwrap_or_else(|| "Chat".into())
    }

    /// Announces `message`, unless it was already announced or is older
    /// than the floor; returns whether it did. Returns at once: the notifier
    /// is called on its own thread.
    pub fn announce(&self, message: &Message) -> bool {
        if !self.claim(message) {
            return false;
        }
        let notification = notification(message, &self.title(&message.conversation_id));
        let services = self.services.clone();
        let started = std::thread::Builder::new()
            .name("chat-notification".into())
            .spawn(move || {
                if let Err(e) =
                    forge_runtime::block_on(services.schedule_notification(notification))
                {
                    // Turned off in Settings, most likely.
                    forge_runtime::logging::info!("message notification not shown: {e}");
                }
            });
        if let Err(e) = started {
            forge_runtime::logging::warn!("message notification not started: {e}");
        }
        true
    }

    /// After a background refresh: announces the unread messages from
    /// others in rooms that are not muted, newest last, at most
    /// [`MAX_PER_REFRESH`]. Returns how many it announced.
    pub fn announce_unread(&self, client: &ChatClient) -> usize {
        let mut fresh = Vec::new();
        for room in client.cached_conversations() {
            self.remember_title(&room.id, &room.title);
            if room.muted {
                continue;
            }
            let read_up_to = room.read_up_to.unwrap_or(i64::MIN);
            fresh.extend(client.cached_messages(&room.id).into_iter().filter(|m| {
                m.status == chat_core::MessageStatus::Sent
                    && !m.deleted
                    && !client.config().is_own(m)
                    && m.sent_at > read_up_to
            }));
        }
        fresh.sort_by_key(|m| m.sent_at);
        let skip = fresh.len().saturating_sub(MAX_PER_REFRESH);
        fresh
            .iter()
            .skip(skip)
            .filter(|message| self.announce(message))
            .count()
    }
}

/// The notification for `message`.
pub fn notification(message: &Message, room_title: &str) -> native::LocalNotification {
    native::LocalNotification {
        id: message.id.clone(),
        title: room_title.to_string(),
        body: summary(message),
        subtitle: Some(message.author.clone()),
        thread_id: Some(message.conversation_id.clone()),
        deep_link: Some(message_link(&message.conversation_id, &message.id)),
        trigger: native::NotificationTrigger::Now,
        category: Some(crate::chat::MESSAGE_NOTIFICATION.into()),
    }
}

/// The text a notification shows: the message, or what it carries.
pub fn summary(message: &Message) -> String {
    if !message.body.trim().is_empty() {
        return message.body.clone();
    }
    match message.attachments.first() {
        Some(a) if a.is_image() => "Photo".into(),
        Some(a) if a.mime.starts_with("audio/") => "Voice message".into(),
        Some(a) => a.name.clone(),
        None => "New message".into(),
    }
}

/// `forgechat://conversation/<room>?message=<id>`, each part percent-encoded,
/// so a room id with `/` stays one path segment.
pub fn message_link(conversation_id: &str, message_id: &str) -> String {
    format!(
        "{LINK_SCHEME}://conversation/{}?message={}",
        encode(conversation_id),
        encode(message_id)
    )
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, sent_at: i64) -> Message {
        Message {
            id: id.into(),
            conversation_id: "general".into(),
            author: "Ana".into(),
            author_id: "u-ana".into(),
            body: String::new(),
            sent_at,
            ..Message::default()
        }
    }

    #[test]
    fn nothing_is_announced_twice_or_from_before_the_app_was_seen() {
        let dir = tempfile::tempdir().unwrap();
        let (services, fakes) = native::NativeServices::for_tests(dir.path().join("services"));
        let alerts = Alerts::open(services.clone(), dir.path().into());
        let now = forge_services::notify::now_ms();
        assert!(!alerts.announce(&message("old", now - 60_000)), "history");
        assert!(alerts.announce(&message("new", now + 1)));
        assert!(!alerts.announce(&message("new", now + 1)), "twice");

        // A new process (a refresh after iOS ended the app) reads the record.
        let again = Alerts::open(services.clone(), dir.path().into());
        assert!(!again.announce(&message("new", now + 1)));
        again.seen_until(now + 10);
        assert!(!again.announce(&message("seen", now + 5)));
        assert!(again.announce(&message("later", now + 20)));

        std::thread::sleep(std::time::Duration::from_millis(200));
        let shown: Vec<_> = fakes
            .notifier
            .scheduled()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(shown, vec!["new", "later"]);
        // A photo with no text says so; the room falls back to "Chat".
        let note = &fakes.notifier.scheduled()[0];
        assert_eq!(note.title, "Chat");
        assert_eq!(note.body, "New message");
    }

    #[test]
    fn links_survive_the_deep_link_parser() {
        let link = message_link("a/b c", "m-1");
        assert_eq!(link, "forgechat://conversation/a%2Fb%20c?message=m-1");
        let parsed = native::parse_deep_link(link).unwrap();
        assert_eq!(parsed.route, vec!["conversation", "a/b c"]);
        assert_eq!(parsed.query_value("message"), Some("m-1"));
    }
}
