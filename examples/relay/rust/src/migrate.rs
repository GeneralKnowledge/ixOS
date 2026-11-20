//! Notes become messages (review step 4, docs/relay-unified-delivery.md),
//! once, on the first open after the update:
//!
//! - a pending note delivery becomes a message delivery with the same id,
//!   and the message `note-<delivery id>` is in the conversation, queued;
//! - a pending files delivery stays queued, and its message
//!   `file-<delivery id>` is in the conversation, always: its name, size
//!   and attachment id (the content hash) come from the delivery itself,
//!   whether or not the history still has it or the sent file is still
//!   there;
//! - notes in the history (sent and received) move into their device's
//!   conversation as `note-h<history id>`, with their times, and leave the
//!   history; those of a device that is no longer listed stay;
//! - received-id memory is kept, so a retry from an older build is still
//!   answered once.
//!
//! [`moves`] reads the record; the conversations take the messages (each
//! once, by id); then [`apply`] changes the record and marks it done
//! (`Record::notes_are_messages`). Relay ending between the two repeats
//! the first half, which adds nothing, and running it again changes
//! nothing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chat_core::{Message, MessageStatus};

use crate::device_chat::{files_attachment, files_message_id, Conversations};
use crate::model::RelayError;
use crate::outbox::Payload;
use crate::record::{Record, SavedItem, SavedStatus};

/// A message for a device's conversation, and where its attachment is.
struct Move {
    pub to: String,
    pub message: Message,
    pub path: Option<PathBuf>,
}

/// Moves notes into conversations; the record is saved after it returns.
pub(crate) fn run(record: &mut Record, c: &mut Conversations<'_>) -> Result<(), RelayError> {
    for m in moves(record) {
        c.import(&m.to, m.message, m.path.as_deref())?;
    }
    apply(record);
    Ok(())
}

/// The message id of a note in the history.
fn history_note_id(id: u64) -> String {
    format!("note-h{id}")
}

/// The message id of a pending note delivery.
fn note_id(delivery: &str) -> String {
    format!("note-{delivery}")
}

/// The devices still listed (not forgotten).
fn listed(record: &Record) -> HashSet<String> {
    record
        .devices
        .listed()
        .map(|d| d.endpoint_id.clone())
        .collect()
}

/// What the conversations need from `record`.
fn moves(record: &Record) -> Vec<Move> {
    let me = (record.name.clone(), format!("relay:{}", record.device_id));
    let listed = listed(record);
    let mut moves = Vec::new();
    for d in &record.deliveries {
        let sent_at = d.queued_ms;
        let ours = |id: String| Message {
            id,
            author: me.0.clone(),
            author_id: me.1.clone(),
            status: MessageStatus::Queued,
            sent_at,
            ..Message::default()
        };
        match &d.payload {
            Payload::Note { text } => moves.push(Move {
                to: d.to.clone(),
                message: Message {
                    body: text.clone(),
                    ..ours(note_id(&d.id))
                },
                path: None,
            }),
            Payload::Files {
                hash,
                name,
                size,
                collection,
                ..
            } => {
                // The delivery itself names what it carries, so every
                // pending files delivery gets its message; the history
                // entry (if it wasn't trimmed) adds where the sent file
                // was.
                let file = record
                    .transfers
                    .iter()
                    .find(|t| t.id == d.transfer)
                    .and_then(|t| match &t.item {
                        SavedItem::File { file, .. } => file.as_deref(),
                        SavedItem::Note { .. } => None,
                    })
                    .map(Path::new);
                let attachment = files_attachment(hash, *collection, name, *size);
                moves.push(Move {
                    to: d.to.clone(),
                    message: Message {
                        attachments: vec![attachment],
                        ..ours(files_message_id(&d.id))
                    },
                    path: file.filter(|p| p.exists()).map(Path::to_path_buf),
                });
            }
            Payload::Message { .. } => {}
        }
    }
    let pending: HashSet<u64> = record.deliveries.iter().map(|d| d.transfer).collect();
    for t in &record.transfers {
        let SavedItem::Note { text } = &t.item else {
            continue;
        };
        if pending.contains(&t.id) || !listed.contains(&t.peer_id) {
            continue;
        }
        let (author, author_id) = if t.sent {
            me.clone()
        } else {
            (t.peer_name.clone(), t.peer_id.clone())
        };
        let status = match &t.status {
            SavedStatus::Failed { .. } if t.sent => MessageStatus::Failed,
            _ => MessageStatus::Sent,
        };
        moves.push(Move {
            to: t.peer_id.clone(),
            message: Message {
                id: history_note_id(t.id),
                author,
                author_id,
                body: text.clone(),
                status,
                sent_at: t.at_ms,
                ..Message::default()
            },
            path: None,
        });
    }
    moves
}

/// Changes `record` once the conversations have the moves: pending notes
/// become message deliveries, the notes that moved leave the history, and
/// the record says it is done.
fn apply(record: &mut Record) {
    record.notes_are_messages = true;
    let mut moved = HashSet::new();
    for d in &mut record.deliveries {
        if let Payload::Note { text } = &d.payload {
            d.payload = Payload::Message {
                id: note_id(&d.id),
                body: text.clone(),
                reply_to: None,
            };
            moved.insert(d.transfer);
            d.transfer = 0;
        }
    }
    let listed = listed(record);
    record.transfers.retain(|t| {
        !(matches!(t.item, SavedItem::Note { .. })
            && (moved.contains(&t.id) || listed.contains(&t.peer_id)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_chat::PeerLogs;
    use crate::outbox::SavedDelivery;
    use crate::record::SavedTransfer;

    const PHONE: &str = "aa11";
    const GONE: &str = "bb22";

    fn note(id: u64, peer: &str, sent: bool, text: &str, status: SavedStatus) -> SavedTransfer {
        SavedTransfer {
            id,
            peer_id: peer.into(),
            peer_name: if peer == PHONE { "phone" } else { "old" }.into(),
            sent,
            item: SavedItem::Note { text: text.into() },
            status,
            at_ms: 1_000 * id as i64,
        }
    }

    /// A record saved before notes were messages: one with every case.
    fn saved(dir: &Path) -> Record {
        let mut r = Record {
            device_id: "me".into(),
            name: "laptop".into(),
            ..Record::default()
        };
        r.devices.trust(PHONE, "phone", 1);
        let file = dir.join("map.txt");
        std::fs::write(&file, b"north").unwrap();
        r.transfers = vec![
            note(1, PHONE, true, "sent", SavedStatus::Done),
            note(2, PHONE, false, "received", SavedStatus::Done),
            note(3, GONE, false, "from a forgotten device", SavedStatus::Done),
            note(4, PHONE, true, "waiting", SavedStatus::Waiting),
            SavedTransfer {
                id: 5,
                peer_id: PHONE.into(),
                peer_name: "phone".into(),
                sent: true,
                item: SavedItem::File {
                    name: "map.txt".into(),
                    size: 5,
                    file: Some(file.to_string_lossy().into_owned()),
                },
                status: SavedStatus::Waiting,
                at_ms: 5_000,
            },
        ];
        r.deliveries = vec![
            SavedDelivery {
                id: "d4".into(),
                to: PHONE.into(),
                transfer: 4,
                payload: Payload::Note {
                    text: "waiting".into(),
                },
                queued_ms: 4_000,
            },
            SavedDelivery {
                id: "d5".into(),
                to: PHONE.into(),
                transfer: 5,
                payload: Payload::Files {
                    hash: "5".repeat(64),
                    collection: false,
                    name: "map.txt".into(),
                    size: 5,
                    hashes: Vec::new(),
                },
                queued_ms: 5_000,
            },
            // Its history entry was trimmed (the history keeps 500).
            files_delivery("d6", 0, "trimmed.pdf", 7),
            // Its sent file can't be read any more.
            files_delivery("d7", 7, "gone.jpg", 9),
        ];
        r.transfers.push(SavedTransfer {
            id: 7,
            peer_id: PHONE.into(),
            peer_name: "phone".into(),
            sent: true,
            item: SavedItem::File {
                name: "gone.jpg".into(),
                size: 9,
                file: Some(dir.join("gone.jpg").to_string_lossy().into_owned()),
            },
            status: SavedStatus::Waiting,
            at_ms: 7_000,
        });
        r
    }

    fn files_delivery(id: &str, transfer: u64, name: &str, size: u64) -> SavedDelivery {
        SavedDelivery {
            id: id.into(),
            to: PHONE.into(),
            transfer,
            payload: Payload::Files {
                hash: id.repeat(32),
                collection: false,
                name: name.into(),
                size,
                hashes: Vec::new(),
            },
            queued_ms: 1_000 * transfer.max(6) as i64,
        }
    }

    #[test]
    fn notes_move_into_conversations_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = saved(dir.path());
        let store = std::sync::Arc::new(crate::store::Store::in_memory());
        let logs = PeerLogs::new(dir.path().join("devices"), store.clone());
        let run = |record: &mut Record| {
            store
                .write(|c| run(record, &mut Conversations::new(c)))
                .unwrap()
        };
        run(&mut record);

        let got = logs.get(PHONE).unwrap().since(None);
        let shown: Vec<(&str, &str, MessageStatus, i64)> = got
            .iter()
            .map(|m| (m.id.as_str(), m.body.as_str(), m.status, m.sent_at))
            .collect();
        assert_eq!(
            shown,
            [
                ("note-h1", "sent", MessageStatus::Sent, 1_000),
                ("note-h2", "received", MessageStatus::Sent, 2_000),
                ("note-d4", "waiting", MessageStatus::Queued, 4_000),
                ("file-d5", "", MessageStatus::Queued, 5_000),
                ("file-d6", "", MessageStatus::Queued, 6_000),
                ("file-d7", "", MessageStatus::Queued, 7_000),
            ]
        );
        // Every pending file has its attachment, from the delivery itself
        // when the history has no entry or the file can't be read.
        let named: Vec<(&str, &str, u64)> = got[3..]
            .iter()
            .map(|m| &m.attachments[0])
            .map(|a| (a.name.as_str(), a.mime.as_str(), a.size))
            .collect();
        assert_eq!(
            named,
            [
                ("map.txt", "text/plain", 5),
                ("trimmed.pdf", "application/pdf", 7),
                ("gone.jpg", "image/jpeg", 9),
            ]
        );
        let ids: Vec<&str> = got[3..]
            .iter()
            .map(|m| m.attachments[0].id.as_str())
            .collect();
        assert_eq!(ids, ["5".repeat(64), "d6".repeat(32), "d7".repeat(32)]);
        assert_eq!(got[0].author_id, "relay:me");
        assert_eq!(got[1].author_id, PHONE);
        assert_eq!(got[3].attachments[0].name, "map.txt");
        assert_eq!(
            logs.get(PHONE).unwrap().file(&got[3].attachments[0].id),
            Some(dir.path().join("map.txt"))
        );

        // The pending note is a message delivery with the same id; the
        // files stay as they were.
        assert_eq!(
            record.deliveries[0].payload,
            Payload::Message {
                id: "note-d4".into(),
                body: "waiting".into(),
                reply_to: None
            }
        );
        assert_eq!(
            (
                record.deliveries[0].id.as_str(),
                record.deliveries[0].transfer
            ),
            ("d4", 0)
        );
        assert!(matches!(
            record.deliveries[1].payload,
            Payload::Files { .. }
        ));
        // Only the forgotten device's note and the file stay in history.
        let left: Vec<u64> = record.transfers.iter().map(|t| t.id).collect();
        assert_eq!(left, [3, 5, 7]);
        assert!(record.notes_are_messages);

        // Again (Relay ended before the record was saved): nothing changes.
        let before = logs.get(PHONE).unwrap().since(None).len();
        let mut again = saved(dir.path());
        run(&mut again);
        assert_eq!(again, record);
        assert_eq!(logs.get(PHONE).unwrap().since(None).len(), before);
        let mut twice = record.clone();
        run(&mut twice);
        assert_eq!(twice, record);
    }
}
