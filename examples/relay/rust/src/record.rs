//! What Relay saves: this device's identity, its trusted devices, what
//! waits for them, and the history of what went across. A value in memory,
//! saved as rows of Relay's database (`store`); older builds saved it as one
//! JSON document, `relay.json`, read once to move it in (`VERSION`).
//! Devices paired the old way (QR code, TCP) were kept with their secrets
//! under `peers`; that field is no longer read, so the next save drops it.

use serde::{Deserialize, Serialize};

use crate::model::{Direction, Item, Transfer, TransferStatus};
use crate::outbox::{ReceivedId, SavedDelivery};
use crate::trust::Devices;

/// The saved document's version.
pub const VERSION: u32 = 1;
/// The history keeps this many transfers; older ones are dropped (their
/// received files stay in the inbox until removed).
pub const MAX_HISTORY: usize = 500;
/// The history's other side for a transfer made with a word code.
pub const CODE_PEER: &str = "code";
pub const CODE_PEER_NAME: &str = "someone with a code";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Record {
    /// Retired (P4): the random id an older build made. Read so its own
    /// messages can be moved to [`crate::device_chat::ME`] once, then
    /// cleared; the endpoint id is the device's only id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "first_id")]
    pub next_id: u64,
    #[serde(default)]
    pub transfers: Vec<SavedTransfer>,
    /// Devices trusted on iroh, and the pairing policy (`trust`).
    #[serde(default)]
    pub devices: Devices,
    /// Items waiting for a trusted device, oldest first (`outbox`).
    #[serde(default)]
    pub deliveries: Vec<SavedDelivery>,
    /// Ids of items received from trusted devices, so a retry isn't saved
    /// twice (`outbox`).
    #[serde(default)]
    pub received_ids: Vec<ReceivedId>,
    /// Notes have moved into conversations (`migrate`, review step 4).
    #[serde(default)]
    pub notes_are_messages: bool,
    /// Saved data that couldn't be read when Relay opened, for the person
    /// to hear about until they dismiss it (`store`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<SavedNotice>,
    /// An upgrade turned Relay's servers on (N2): said once, until the
    /// person dismisses it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub away_notice: bool,
    /// Group chats this device is in or was in (`group`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<crate::group::SavedGroup>,
}

/// What couldn't be read, in words, and where it is kept on this device.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedNotice {
    pub message: String,
    pub kept: Vec<String>,
}

fn first_id() -> u64 {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedTransfer {
    pub id: u64,
    pub peer_id: String,
    pub peer_name: String,
    pub sent: bool,
    pub item: SavedItem,
    pub status: SavedStatus,
    pub at_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SavedItem {
    Note {
        text: String,
    },
    /// `file` is the inbox name of a received file, or the sent file's
    /// path.
    File {
        name: String,
        size: u64,
        file: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SavedStatus {
    /// Queued for a trusted device that hasn't been reached yet.
    Waiting,
    InProgress,
    Done,
    Failed {
        reason: String,
    },
}

impl Record {
    /// Starts a history entry; returns its id.
    pub fn begin(&mut self, peer_id: &str, sent: bool, item: SavedItem, now_ms: i64) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let peer_name = self
            .devices
            .get(peer_id)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        self.transfers.push(SavedTransfer {
            id,
            peer_id: peer_id.into(),
            peer_name,
            sent,
            item,
            status: SavedStatus::InProgress,
            at_ms: now_ms,
        });
        if self.transfers.len() > MAX_HISTORY {
            let extra = self.transfers.len() - MAX_HISTORY;
            self.transfers.drain(..extra);
        }
        id
    }

    pub fn finish(&mut self, id: u64, status: SavedStatus, file: Option<String>) {
        if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
            t.status = status;
            if let (SavedItem::File { file: slot, .. }, Some(file)) = (&mut t.item, file) {
                *slot = Some(file);
            }
        }
    }

    /// Adds a finished transfer made with a word code: the other side is
    /// "someone with a code", never a trusted device.
    pub fn code_transfer(
        &mut self,
        sent: bool,
        item: SavedItem,
        status: SavedStatus,
        now_ms: i64,
    ) -> u64 {
        let id = self.begin(CODE_PEER, sent, item, now_ms);
        if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
            t.peer_name = CODE_PEER_NAME.into();
            t.status = status;
        }
        id
    }

    /// Transfers that were running when the app ended didn't finish; those
    /// still in the outbox wait to be sent again.
    pub fn interrupted(&mut self) -> bool {
        let mut any = false;
        let queued: std::collections::HashSet<u64> =
            self.deliveries.iter().map(|d| d.transfer).collect();
        for t in &mut self.transfers {
            if t.status == SavedStatus::InProgress && queued.contains(&t.id) {
                t.status = SavedStatus::Waiting;
                any = true;
            } else if t.status == SavedStatus::InProgress {
                t.status = SavedStatus::Failed {
                    reason: "Relay closed before it finished".into(),
                };
                any = true;
            }
        }
        any
    }

    /// The history, newest first. `inbox` turns a received file's name into
    /// a path (or `None` if it is gone); `progress` gives bytes so far.
    /// A waiting send left in the recipient's mailbox at `now_ms` is
    /// [`TransferStatus::InMailbox`].
    pub fn transfers(
        &self,
        inbox: impl Fn(&str) -> Option<String>,
        progress: impl Fn(u64) -> u64,
        now_ms: i64,
    ) -> Vec<Transfer> {
        self.transfers
            .iter()
            .rev()
            .map(|t| Transfer {
                id: t.id,
                peer_id: t.peer_id.clone(),
                peer_name: t.peer_name.clone(),
                direction: if t.sent {
                    Direction::Sent
                } else {
                    Direction::Received
                },
                item: match &t.item {
                    SavedItem::Note { text } => Item::Note { text: text.clone() },
                    SavedItem::File { name, size, file } => Item::File {
                        name: name.clone(),
                        size: *size,
                        path: match file {
                            Some(f) if t.sent => {
                                std::path::Path::new(f).exists().then(|| f.clone())
                            }
                            Some(f) => inbox(f),
                            None => None,
                        },
                    },
                },
                status: match &t.status {
                    SavedStatus::Waiting if t.sent && self.sent_to_mailbox(t.id, now_ms) => {
                        TransferStatus::InMailbox
                    }
                    SavedStatus::Waiting => TransferStatus::Waiting,
                    SavedStatus::InProgress => TransferStatus::InProgress {
                        done: progress(t.id),
                    },
                    SavedStatus::Done => TransferStatus::Done,
                    SavedStatus::Failed { reason } => TransferStatus::Failed {
                        reason: reason.clone(),
                    },
                },
                at_ms: t.at_ms,
            })
            .collect()
    }
}
