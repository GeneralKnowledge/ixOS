//! What Relay saves: this device's identity, its paired devices with their
//! secrets, and the history of what went across. One JSON document.

use serde::{Deserialize, Serialize};

use crate::model::{Direction, Item, Peer, Transfer, TransferStatus};
use crate::wire::{hex, unhex};

/// The saved document's version.
pub const VERSION: u32 = 1;
/// The history keeps this many transfers; older ones are dropped (their
/// received files stay in the inbox until removed).
pub const MAX_HISTORY: usize = 500;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Record {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "first_id")]
    pub next_id: u64,
    #[serde(default)]
    pub peers: Vec<SavedPeer>,
    #[serde(default)]
    pub transfers: Vec<SavedTransfer>,
}

fn first_id() -> u64 {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedPeer {
    pub device_id: String,
    pub name: String,
    /// The pair's shared secret, hex. In the app's data container, which iOS
    /// encrypts while the phone is locked (friction R2: Keychain later).
    pub secret: String,
    pub paired_ms: i64,
    #[serde(default)]
    pub last_address: Option<String>,
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
    InProgress,
    Done,
    Failed { reason: String },
}

impl Record {
    pub fn peer(&self, device_id: &str) -> Option<&SavedPeer> {
        self.peers.iter().find(|p| p.device_id == device_id)
    }

    pub fn secret(&self, device_id: &str) -> Option<[u8; 32]> {
        self.peer(device_id)
            .and_then(|p| unhex(&p.secret))
            .and_then(|s| s.try_into().ok())
    }

    /// Adds or refreshes a paired device (pairing again replaces the secret).
    pub fn pair(
        &mut self,
        device_id: &str,
        name: &str,
        secret: &[u8; 32],
        address: Option<String>,
        now_ms: i64,
    ) {
        self.peers.retain(|p| p.device_id != device_id);
        self.peers.push(SavedPeer {
            device_id: device_id.into(),
            name: name.into(),
            secret: hex(secret),
            paired_ms: now_ms,
            last_address: address,
        });
    }

    pub fn reached(&mut self, device_id: &str, name: &str, address: Option<String>) {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.device_id == device_id) {
            peer.name = name.into();
            if address.is_some() {
                peer.last_address = address;
            }
        }
    }

    /// Starts a history entry; returns its id.
    pub fn begin(&mut self, peer_id: &str, sent: bool, item: SavedItem, now_ms: i64) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let peer_name = self
            .peer(peer_id)
            .map(|p| p.name.clone())
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

    /// Transfers that were running when the app ended didn't finish.
    pub fn interrupted(&mut self) -> bool {
        let mut any = false;
        for t in &mut self.transfers {
            if t.status == SavedStatus::InProgress {
                t.status = SavedStatus::Failed {
                    reason: "Relay closed before it finished".into(),
                };
                any = true;
            }
        }
        any
    }

    pub fn peers(&self) -> Vec<Peer> {
        let mut peers: Vec<Peer> = self
            .peers
            .iter()
            .map(|p| Peer {
                device_id: p.device_id.clone(),
                name: p.name.clone(),
                paired_ms: p.paired_ms,
                last_address: p.last_address.clone(),
            })
            .collect();
        peers.sort_by_key(|p| p.name.to_lowercase());
        peers
    }

    /// The history, newest first. `inbox` turns a received file's name into
    /// a path (or `None` if it is gone); `progress` gives bytes so far.
    pub fn transfers(
        &self,
        inbox: impl Fn(&str) -> Option<String>,
        progress: impl Fn(u64) -> u64,
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
