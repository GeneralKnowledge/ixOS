//! The delivery outbox (Relay M1 stage 3,
//! examples/relay/docs/relay-reliable-delivery.md and
//! relay-delivery-outbox.md; one queue since review step 4,
//! relay-unified-delivery.md): conversation messages and files for a
//! trusted device wait here until it is reachable, survive a relaunch, and
//! arrive once, in the order they were sent.
//!
//! - **The queue.** Every item sent to a trusted device is saved in
//!   `Record::deliveries` before anything is dialled, with a 128-bit id it
//!   keeps through every retry and relaunch. A file's bytes are copied into
//!   the blob store when it is queued and held there by one named tag,
//!   `relay/delivery/<id>`, until the delivery ends.
//! - **The scheduler** is shared by both kinds. One worker per recipient
//!   takes its items oldest first and never overtakes one that is waiting,
//!   so a recipient gets them in the order they were sent. It runs when an
//!   item is queued, when the app sees the device or the device connects
//!   here (which also resets the backoff), when Relay comes to the
//!   foreground ([`RelayCore::deliver_waiting`]), and on a backoff of 1 s
//!   doubling to 5 minutes, with jitter (forge-net's `Backoff`).
//! - **Two lifecycles.** A message is Queued in its conversation until the
//!   device has it, then Sent; it has no history entry. A file is Waiting,
//!   then Sending while the recipient fetches what it doesn't have yet (so
//!   a retry resumes), then Done once the recipient has verified and saved
//!   it. Either ends Failed with a cause: refused or declined by the
//!   recipient, no longer trusted here, cancelled, or not delivered within
//!   30 days. A failed message can be sent again from its conversation.
//!   (Notes queued before step 4 still go as notes.)
//! - **Exactly once.** The recipient saves an item, then remembers its id
//!   for 45 days, longer than any sender keeps it pending (30), before it
//!   answers. A retry whose answer was lost finds the id and is answered
//!   Done without saving anything again.
//! - Only while Relay runs: nothing is delivered in the background.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use forge_net::Backoff;
use forge_runtime::{lock, now_ms};
use iroh_blobs::{Hash, HashAndFormat};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::code_send::Offer;
use crate::device_chat::Conversations;
use crate::hex::{hex, unhex};
use crate::model::{RelayError, Transfer};
use crate::record::{Record, SavedItem, SavedStatus};
use crate::resolver::{self, Whereabouts};
use crate::trust::{SavedDevice, SavedTrust};
use crate::{code, device, RelayCore};

/// A pending item ends unsent after this long.
pub(crate) const PENDING_KEPT_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// The recipient remembers delivered ids this long: longer than any
/// sender keeps an item pending, so no retry outlives the memory of it.
pub(crate) const RECEIVED_KEPT_MS: i64 = 45 * 24 * 60 * 60 * 1000;
const BACKOFF_FIRST: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);
const TAG: &str = "relay/delivery/";

/// One item waiting for a trusted device, in `Record::deliveries` in the
/// order it was queued. It leaves the list when it ends; its history entry
/// keeps how.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedDelivery {
    /// 16 random bytes, hex: the id on the wire, kept through retries.
    pub id: String,
    /// The recipient's endpoint id.
    pub to: String,
    /// Its history entry; 0 for a message, which is in its device's
    /// conversation instead.
    pub transfer: u64,
    pub payload: Payload,
    pub queued_ms: i64,
    /// When it was left in the recipient's mailbox (N3), if it was: it
    /// waits there for a receipt, and goes again only once the mailbox
    /// has let it expire, or when the device is reachable again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Payload {
    Note {
        text: String,
    },
    /// A message in the device's conversation, by its own id there (the
    /// receiver keeps it once by that id).
    Message {
        id: String,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
    },
    /// Files and folders as one item, in the blob store: a raw blob, or a
    /// collection. `hashes` are every blob the recipient may fetch.
    Files {
        hash: String,
        collection: bool,
        name: String,
        size: u64,
        hashes: Vec<String>,
    },
}

/// An item this device received from a trusted device, so a retry of it
/// is answered without saving it again.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReceivedId {
    pub from: String,
    pub id: String,
    pub at_ms: i64,
}

impl SavedDelivery {
    /// Whether it waits in the recipient's mailbox at `now_ms`.
    pub fn in_mailbox(&self, now_ms: i64) -> bool {
        self.left_ms
            .is_some_and(|left| now_ms - left < crate::mailbox_server::KEPT_MS)
    }
}

impl Payload {
    pub(crate) fn files(offer: &Offer) -> Self {
        let mut hashes: Vec<String> = offer.hashes.iter().map(|h| hex(h.as_bytes())).collect();
        hashes.sort();
        Payload::Files {
            hash: hex(offer.hash.as_bytes()),
            collection: offer.collection,
            name: offer.name.clone(),
            size: offer.size,
            hashes,
        }
    }

    fn offer(&self) -> Option<Offer> {
        let Payload::Files {
            hash,
            collection,
            name,
            size,
            hashes,
        } = self
        else {
            return None;
        };
        Some(Offer {
            hash: parse_hash(hash)?,
            collection: *collection,
            size: *size,
            name: name.clone(),
            hashes: hashes.iter().filter_map(|h| parse_hash(h)).collect(),
        })
    }

    /// What the blob store holds for it.
    fn content(&self) -> Option<HashAndFormat> {
        match self {
            Payload::Note { .. } | Payload::Message { .. } => None,
            Payload::Files {
                hash, collection, ..
            } => {
                let hash = parse_hash(hash)?;
                Some(if *collection {
                    HashAndFormat::hash_seq(hash)
                } else {
                    HashAndFormat::raw(hash)
                })
            }
        }
    }
}

fn parse_hash(text: &str) -> Option<Hash> {
    let bytes: [u8; 32] = unhex(text)?.try_into().ok()?;
    Some(Hash::from_bytes(bytes))
}

pub(crate) fn new_id() -> [u8; 16] {
    crate::hex::random()[..16].try_into().expect("16 bytes")
}

pub(crate) fn tag_name(id: &str) -> String {
    format!("{TAG}{id}")
}

/// A recipient's backoff: 1 s doubling to 5 minutes, up to a fifth
/// shorter at random (forge-net's `Backoff`).
fn backoff() -> Backoff {
    Backoff::new(BACKOFF_FIRST, BACKOFF_MAX).with_jitter(0.2)
}

impl Record {
    /// Queues an item for `to`, with a history entry that says it is
    /// waiting; returns the entry's id.
    pub fn queue(
        &mut self,
        id: &[u8; 16],
        to: &str,
        item: SavedItem,
        payload: Payload,
        now_ms: i64,
    ) -> u64 {
        let transfer = self.begin(to, true, item, now_ms);
        self.finish(transfer, SavedStatus::Waiting, None);
        self.deliveries.push(SavedDelivery {
            id: hex(id),
            to: to.into(),
            transfer,
            payload,
            queued_ms: now_ms,
            left_ms: None,
        });
        transfer
    }

    /// Queues a message for `to`. It has no history entry: it is in the
    /// device's conversation.
    pub fn queue_message(&mut self, id: &[u8; 16], to: &str, payload: Payload, now_ms: i64) {
        self.deliveries.push(SavedDelivery {
            id: hex(id),
            to: to.into(),
            transfer: 0,
            payload,
            queued_ms: now_ms,
            left_ms: None,
        });
    }

    /// The pending delivery of the conversation message `message_id`.
    pub fn message_delivery(&self, message_id: &str) -> Option<&SavedDelivery> {
        self.deliveries.iter().find(|d| match &d.payload {
            Payload::Message { id, .. } => id == message_id,
            Payload::Files { .. } => crate::device_chat::files_message_id(&d.id) == message_id,
            Payload::Note { .. } => false,
        })
    }

    /// The oldest item for `to`: the next to send.
    pub fn next_for(&self, to: &str) -> Option<&SavedDelivery> {
        self.deliveries.iter().find(|d| d.to == to)
    }

    /// The oldest item for `to` that isn't waiting in its mailbox (one
    /// the mailbox has let expire is due again), or with `direct`, the
    /// oldest of all: the device may be reachable again.
    pub fn next_due(&self, to: &str, direct: bool, now_ms: i64) -> Option<&SavedDelivery> {
        self.deliveries
            .iter()
            .find(|d| d.to == to && (direct || !d.in_mailbox(now_ms)))
    }

    /// Devices with an item the mailbox has let expire, once each: they
    /// are due to be tried again.
    pub fn left_expired(&self, now_ms: i64) -> Vec<String> {
        let mut seen = HashSet::new();
        self.deliveries
            .iter()
            .filter(|d| d.left_ms.is_some() && !d.in_mailbox(now_ms))
            .filter(|d| seen.insert(d.to.clone()))
            .map(|d| d.to.clone())
            .collect()
    }

    /// Marks a pending delivery as left in its recipient's mailbox.
    pub fn left_in_mailbox(&mut self, id: &str, now_ms: i64) -> bool {
        match self.deliveries.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.left_ms = Some(now_ms);
                true
            }
            None => false,
        }
    }

    pub fn delivery(&self, id: &str) -> Option<&SavedDelivery> {
        self.deliveries.iter().find(|d| d.id == id)
    }

    /// Recipients with something waiting.
    pub fn waiting_for(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.deliveries
            .iter()
            .filter(|d| seen.insert(d.to.clone()))
            .map(|d| d.to.clone())
            .collect()
    }

    /// Whether the send for history entry `transfer` waits sealed in the
    /// recipient's mailbox at `now_ms`, out of this device's hands.
    pub fn sent_to_mailbox(&self, transfer: u64, now_ms: i64) -> bool {
        self.deliveries
            .iter()
            .any(|d| d.transfer == transfer && d.in_mailbox(now_ms))
    }

    /// Ends a delivery: its history entry takes `status`. False when it had
    /// already ended (cancelled while it was being sent).
    pub fn end_delivery(&mut self, id: &str, status: SavedStatus) -> bool {
        let Some(i) = self.deliveries.iter().position(|d| d.id == id) else {
            return false;
        };
        let d = self.deliveries.remove(i);
        self.finish(d.transfer, status, None);
        true
    }

    /// Sets a pending delivery's history status (Waiting or InProgress).
    /// False when it has ended.
    pub fn delivery_status(&mut self, id: &str, status: SavedStatus) -> bool {
        let Some(transfer) = self.delivery(id).map(|d| d.transfer) else {
            return false;
        };
        self.finish(transfer, status, None);
        true
    }

    /// Whether this device already has the item `id` from `from`.
    pub fn received_before(&self, from: &str, id: &[u8; 16]) -> bool {
        let id = hex(id);
        self.received_ids
            .iter()
            .any(|r| r.from == from && r.id == id)
    }

    /// Remembers a received item's id, and forgets ids older than 45 days.
    pub fn remember_received(&mut self, from: &str, id: &[u8; 16], now_ms: i64) {
        self.received_ids
            .retain(|r| now_ms - r.at_ms < RECEIVED_KEPT_MS);
        if !self.received_before(from, id) {
            self.received_ids.push(ReceivedId {
                from: from.into(),
                id: hex(id),
                at_ms: now_ms,
            });
        }
    }
}

type Waiter = oneshot::Sender<Result<(), RelayError>>;

/// The scheduler's state in the core. Nothing here is saved: after a
/// relaunch every recipient is due at once.
#[derive(Default)]
pub(crate) struct Outbox {
    /// Recipients with a worker running.
    draining: Mutex<HashSet<String>>,
    /// Per recipient that wasn't reached: its backoff, and when the next
    /// pass is due.
    backoff: Mutex<HashMap<String, (Backoff, i64)>>,
    /// Sends waiting for their item's first attempt, by delivery id.
    pub(crate) waiters: Arc<Mutex<HashMap<String, Vec<Waiter>>>>,
    /// Tests: the next answers to lose, as if the connection broke after
    /// the recipient saved the item.
    lose_answers: AtomicU32,
    /// Recipients whose items in their mailbox are tried directly again:
    /// they were seen, connected, or Relay came to the foreground.
    direct_again: Mutex<HashSet<String>>,
    /// The mailbox collector (N3): started once, and woken to collect now.
    pub(crate) collector_started: AtomicBool,
    pub(crate) collect_now: Arc<tokio::sync::Notify>,
    /// History entries whose item is being left in the mailbox right now:
    /// from the moment that is decided (in a command, so it orders with a
    /// cancel) until `left_ms` is saved. They can't be cancelled either.
    depositing: Arc<Mutex<HashSet<u64>>>,
}

/// How one attempt went.
enum Outcome {
    Delivered(device::Sent),
    /// It ends unsent: the cause for the history and the error for the
    /// send that queued it.
    Ended(String, RelayError),
    /// The device wasn't reached; it waits.
    Retry,
    /// It was cancelled before it was sent.
    Gone,
}

impl RelayCore {
    /// Queues an item for a trusted device and waits for its first attempt
    /// (none when an earlier item for the device is waiting). Returns the
    /// history entry: Done, or Waiting. A refusal is an error.
    pub(crate) async fn queue(
        &self,
        id: [u8; 16],
        to: String,
        item: SavedItem,
        payload: Payload,
    ) -> Result<Transfer, RelayError> {
        self.queue_with(id, to, item, payload, None, None).await
    }

    /// [`queue`](Self::queue), with `message` (and where its attachment
    /// is) added to the device's conversation in the same transaction.
    pub(crate) async fn queue_with(
        &self,
        id: [u8; 16],
        to: String,
        item: SavedItem,
        payload: Payload,
        message: Option<(chat_core::Message, Option<std::path::PathBuf>)>,
        kept: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Transfer, RelayError> {
        let (tx, rx) = oneshot::channel();
        let waiters = Arc::clone(&self.outbox.waiters);
        let recipient = to.clone();
        let transfer = self
            .rt
            .command("queue", move |state| {
                state.change_with(|r, c| {
                    if let Some((message, path)) = message {
                        c.add_files(&recipient, message, path.as_deref())?;
                    }
                    let transfer = r.queue(&id, &recipient, item, payload, now_ms());
                    // Before the change is published, so the worker can't
                    // finish the item before anyone waits for it.
                    lock(&waiters).entry(hex(&id)).or_default().push(tx);
                    Ok(transfer)
                })
            })
            .await?;
        if let Some(kept) = kept {
            kept.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.wake(&to);
        rx.await.unwrap_or(Ok(()))?;
        self.transfers()
            .into_iter()
            .find(|t| t.id == transfer)
            .ok_or(RelayError::NotFound { id: transfer })
    }

    /// Tries `to`'s queue now, forgetting its backoff: it was seen, it
    /// connected, or Relay came back to the foreground.
    pub(crate) fn try_now(&self, to: &str) {
        let waker = lock(&self.chat_waker).clone();
        if let Some(wake) = waker {
            wake(to);
        }
        if self.record.get().next_for(to).is_none() {
            return;
        }
        lock(&self.outbox.direct_again).insert(to.to_string());
        lock(&self.outbox.backoff).remove(to);
        self.wake(to);
    }

    /// Starts `to`'s worker unless it is running; a running worker reads
    /// the queue again before it stops.
    pub(crate) fn wake(&self, to: &str) {
        if !lock(&self.outbox.draining).insert(to.to_string()) {
            return;
        }
        let me = self.me.clone();
        let to = to.to_string();
        code::TOKIO.spawn(drain(me, to));
    }

    /// The next item to try for `to`, when one is due. `None` ends the
    /// worker: called with `draining` held, so a wake can't slip between.
    fn due(&self, to: &str) -> Option<SavedDelivery> {
        if let Some((_, next)) = lock(&self.outbox.backoff).get(to) {
            if *next > now_ms() {
                return None;
            }
        }
        let direct = lock(&self.outbox.direct_again).contains(to);
        self.record.get().next_due(to, direct, now_ms()).cloned()
    }

    /// Answers the sends still waiting on `to`'s items: they wait.
    fn settle(&self, to: &str) {
        let record = self.record.get();
        let mut waiters = lock(&self.outbox.waiters);
        for d in record.deliveries.iter().filter(|d| d.to == to) {
            for tx in waiters.remove(&d.id).unwrap_or_default() {
                let _ = tx.send(Ok(()));
            }
        }
    }

    fn answer(&self, delivery: &str, result: Result<(), RelayError>) {
        for tx in lock(&self.outbox.waiters)
            .remove(delivery)
            .unwrap_or_default()
        {
            let _ = tx.send(result.clone());
        }
    }

    /// One attempt at `d`; returns false when the device wasn't reached.
    async fn attempt(&self, d: SavedDelivery) -> bool {
        let outcome = self.try_once(&d).await;
        if matches!(outcome, Outcome::Retry) && self.waits_in_mailbox(&d).await {
            // It waits in the mailbox: the next item goes on.
            self.answer(&d.id, Ok(()));
            return false;
        }
        let reached = !matches!(outcome, Outcome::Retry);
        let (status, answer, seen) = match outcome {
            Outcome::Gone => return true,
            Outcome::Delivered(sent) => (SavedStatus::Done, Ok(()), Some(sent)),
            Outcome::Ended(reason, err) => (SavedStatus::Failed { reason }, Err(err), None),
            Outcome::Retry => (SavedStatus::Waiting, Ok(()), None),
        };
        let (id, to) = (d.id.clone(), d.to.clone());
        let (d_for_end, delivered) = (d.clone(), answer.is_ok());
        let delivered_now = seen.is_some();
        let saved = self
            .rt
            .command("delivery", move |state| {
                // The delivery's end and its message's status: one
                // transaction (P3).
                state.change_with(|r, c| {
                    if let Some(sent) = seen {
                        remember_reached(r, &to, &sent);
                    }
                    if status == SavedStatus::Waiting {
                        r.delivery_status(&id, status);
                        return Ok(false);
                    }
                    let ended = r.end_delivery(&id, status);
                    if ended {
                        message_ended(c, &d_for_end, delivered)?;
                    }
                    Ok(ended)
                })
            })
            .await;
        if let Err(err) = &saved {
            forge_runtime::logging::warn!("relay couldn't save a delivery: {err}");
        }
        if matches!(saved, Ok(true)) {
            self.let_go(&d).await;
        }
        if delivered_now {
            // It has just been told this device's pass (in the Hello).
            self.collect_mailbox();
        }
        self.answer(&d.id, answer);
        let mut backoff = lock(&self.outbox.backoff);
        if reached {
            lock(&self.problems).remove(&d.to);
            backoff.remove(&d.to);
        } else {
            let (b, next) = backoff
                .entry(d.to.clone())
                .or_insert_with(|| (self::backoff(), 0));
            let wait = b.next_delay().as_millis() as i64;
            *next = now_ms() + wait;
            let (me, to) = (self.me.clone(), d.to.clone());
            code::TOKIO.spawn(async move {
                tokio::time::sleep(Duration::from_millis(wait as u64)).await;
                if let Some(core) = me.upgrade() {
                    core.wake(&to);
                }
            });
        }
        reached
    }

    async fn try_once(&self, d: &SavedDelivery) -> Outcome {
        if now_ms() - d.queued_ms > PENDING_KEPT_MS {
            let reason = "Not sent: the other device wasn't seen for 30 days";
            return Outcome::Ended(
                reason.into(),
                RelayError::Unreachable {
                    reason: reason.into(),
                },
            );
        }
        let saved = self.record.get().devices.get(&d.to).cloned();
        let Some(saved) = saved.filter(|s| s.trust == SavedTrust::Trusted) else {
            return Outcome::Ended(
                "No longer trusted".into(),
                RelayError::UnknownPeer { id: d.to.clone() },
            );
        };
        let Some((addr, plan)) = self.dial(&saved) else {
            return Outcome::Retry;
        };
        let outgoing = match &d.payload {
            Payload::Note { text } => device::Outgoing::Note(text.clone()),
            Payload::Message { id, body, reply_to } => {
                device::Outgoing::Message(crate::device_chat::PeerMessage {
                    id: id.clone(),
                    body: body.clone(),
                    reply_to: reply_to.clone(),
                })
            }
            files => match files.offer() {
                Some(offer) => device::Outgoing::Stored(offer),
                None => {
                    let reason = "Relay lost what it was keeping to send";
                    return Outcome::Ended(
                        reason.into(),
                        RelayError::File {
                            reason: reason.into(),
                        },
                    );
                }
            },
        };
        let id: [u8; 16] = unhex(&d.id)
            .and_then(|b| b.try_into().ok())
            .unwrap_or_else(new_id);
        let (delivery, transfer) = (d.id.clone(), d.transfer);
        let sending = self
            .rt
            .command("delivery-sending", move |state| {
                state.change(|r| Ok(r.delivery_status(&delivery, SavedStatus::InProgress)))
            })
            .await;
        if !matches!(sending, Ok(true)) {
            // Cancelled meanwhile: nothing to send.
            return Outcome::Gone;
        }
        let stop = Arc::new(AtomicBool::new(false));
        // A message has no history entry: nothing shows its progress, and
        // it isn't stopped once on its way.
        if transfer != 0 {
            lock(&self.device_stops).insert(transfer, Arc::clone(&stop));
        }
        let node = Arc::clone(&self.node);
        let mode = self.record.get().devices.mode;
        let my_name = self.identity().name;
        let weak = self.me.clone();
        let result = code::TOKIO
            .spawn(async move {
                let node = node.get().await?;
                let report = |done: u64| {
                    if transfer == 0 {
                        return;
                    }
                    if let Some(core) = weak.upgrade() {
                        lock(&core.progress).insert(transfer, done);
                    }
                };
                device::send(&node, addr, mode, my_name, id, outgoing, report, stop).await
            })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)));
        if transfer != 0 {
            lock(&self.device_stops).remove(&transfer);
            lock(&self.progress).remove(&transfer);
        }
        let result = match result {
            Ok(_) if self.lose_answer() => Err(code::connection("the answer was lost")),
            other => other,
        };
        match result {
            Ok(sent) => Outcome::Delivered(sent),
            Err(RelayError::Unreachable { .. } | RelayError::Connection { .. }) => {
                let servers_down = self.server_health() == resolver::ServerHealth::Unreachable;
                self.problem(
                    &saved.endpoint_id,
                    resolver::no_answer(&saved.name, &plan, servers_down),
                );
                Outcome::Retry
            }
            Err(err @ RelayError::Stopped { .. }) => Outcome::Ended("Cancelled".into(), err),
            Err(err) => Outcome::Ended(err.saved_reason(), err),
        }
    }

    fn lose_answer(&self) -> bool {
        self.outbox
            .lose_answers
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }

    /// Lets go of what the store kept for an ended delivery.
    async fn let_go(&self, d: &SavedDelivery) {
        if d.payload.content().is_none() {
            return;
        }
        let (node, name) = (Arc::clone(&self.node), tag_name(&d.id));
        let _ = code::TOKIO
            .spawn(async move {
                if let Ok(node) = node.get().await {
                    if let Err(err) = node.store.tags().delete(name).await {
                        log::warn!("relay couldn't let go of a delivery: {err}");
                    }
                }
            })
            .await;
    }

    /// Takes a waiting item off the queue; one being sent is stopped and
    /// ends Cancelled. False when `transfer` isn't pending, or when it is
    /// already sealed in the recipient's mailbox: the mailbox has no
    /// withdraw, so the recipient gets it whatever this device does, and
    /// calling it Cancelled would be untrue.
    pub(crate) async fn cancel_delivery(&self, transfer: u64) -> bool {
        if self.out_of_hands(transfer) {
            return false;
        }
        if let Some(stop) = lock(&self.device_stops).get(&transfer) {
            stop.store(true, Ordering::SeqCst);
            return true;
        }
        let Some(d) = self
            .record
            .get()
            .deliveries
            .iter()
            .find(|d| d.transfer == transfer)
            .cloned()
        else {
            return false;
        };
        let ending = d.clone();
        let depositing = Arc::clone(&self.outbox.depositing);
        let ended = self
            .rt
            .command("cancel-delivery", move |state| {
                state.change_with(|r, c| {
                    // Checked again here: it may have been left in the
                    // mailbox since the check above.
                    if r.sent_to_mailbox(ending.transfer, now_ms())
                        || lock(&depositing).contains(&ending.transfer)
                    {
                        return Ok(false);
                    }
                    let ended = r.end_delivery(
                        &ending.id,
                        SavedStatus::Failed {
                            reason: "Cancelled".into(),
                        },
                    );
                    if ended {
                        message_ended(c, &ending, false)?;
                    }
                    Ok(ended)
                })
            })
            .await;
        let ended = matches!(ended, Ok(true));
        if ended {
            self.let_go(&d).await;
            self.answer(
                &d.id,
                Err(RelayError::Stopped {
                    kind: forge_app::ErrorKind::Cancelled,
                }),
            );
        }
        ended
    }
}

/// A message or files delivery ended: its conversation shows it sent, or
/// failed (the person can send a message again). In the transaction that
/// ends the delivery.
fn message_ended(
    c: &mut Conversations<'_>,
    d: &SavedDelivery,
    delivered: bool,
) -> Result<(), RelayError> {
    let id = match &d.payload {
        Payload::Message { id, .. } => id.clone(),
        Payload::Files { .. } => crate::device_chat::files_message_id(&d.id),
        Payload::Note { .. } => return Ok(()),
    };
    let status = if delivered {
        chat_core::MessageStatus::Sent
    } else {
        chat_core::MessageStatus::Failed
    };
    c.set_status(&d.to, &id, status)
}

/// At open: a message a conversation shows queued that the outbox doesn't
/// hold failed, so the person can send it again. Since P3 the two are
/// saved together; this settles what an older build left.
pub(crate) fn settle_messages(r: &Record, c: &mut Conversations<'_>) {
    for to in r.devices.listed().map(|d| d.endpoint_id.clone()) {
        for m in c.queued(&to).unwrap_or_default() {
            if r.message_delivery(&m.id).is_none() {
                let _ = c.set_status(&to, &m.id, chat_core::MessageStatus::Failed);
            }
        }
    }
}

/// A delivery reached `to`: remember where, what it said about itself,
/// and the route.
fn remember_reached(r: &mut Record, to: &str, sent: &device::Sent) {
    let at: Vec<String> = sent.at.iter().map(|a| a.to_string()).collect();
    r.devices.seen_at(to, &at, now_ms());
    r.devices.met(
        to,
        &sent.addresses,
        sent.route,
        sent.mailbox.clone(),
        now_ms(),
    );
}

impl RelayCore {
    /// Where to dial a trusted device, by the resolver: what Bonjour sees
    /// now, what it told this device, where it was last reached, limited
    /// by the connection mode. `None` (with the reason kept for the app)
    /// when there is nothing to try.
    pub(crate) fn dial(&self, saved: &SavedDevice) -> Option<(iroh::EndpointAddr, resolver::Plan)> {
        let nearby = lock(&self.nearby)
            .get(&saved.endpoint_id)
            .cloned()
            .unwrap_or_default();
        let mode = self.record.get().devices.mode;
        let w = Whereabouts {
            name: &saved.name,
            nearby: &nearby,
            told: &saved.told,
            remembered: &saved.addresses,
            by_key: self.record.get().devices.servers.finds_by_key(),
        };
        let plan = match resolver::plan(&w, mode) {
            Ok(plan) => plan,
            Err(reason) => {
                self.problem(&saved.endpoint_id, reason);
                return None;
            }
        };
        match device::endpoint_addr(&saved.endpoint_id, &plan.addresses) {
            Ok(addr) => Some((addr, plan)),
            Err(err) => {
                self.problem(&saved.endpoint_id, err.to_string());
                None
            }
        }
    }

    /// Tries everything that waits now, forgetting the backoff: the
    /// network changed, so routes that failed may work.
    pub(crate) fn retry_now(&self) {
        lock(&self.outbox.backoff).clear();
        self.deliver_waiting();
    }

    /// Watches this device's addresses: when they change (another Wi-Fi,
    /// Wi-Fi to mobile data, Tailscale coming on), Relay retries at once.
    /// iroh notices the change itself and rebinds; this is what tells the
    /// outbox. Started once, with the node.
    pub(crate) fn watch_network(&self, node: &crate::node::Node) {
        if self.watching_network.swap(true, Ordering::SeqCst) {
            return;
        }
        self.start_collector();
        use iroh::Watcher;
        let me = self.me.clone();
        let mut watcher = node.endpoint.watch_addr();
        // Learned before anything is dialled, so the first delivery already
        // knows which networks are this device's own.
        let owner = node.endpoint.id().to_string();
        let mut last: HashSet<std::net::SocketAddr> = watcher.get().ip_addrs().copied().collect();
        crate::resolver::learn_own_addresses(&owner, last.iter());
        code::TOKIO.spawn(async move {
            while watcher.updated().await.is_ok() {
                let now: HashSet<std::net::SocketAddr> =
                    watcher.get().ip_addrs().copied().collect();
                if now == last {
                    continue;
                }
                crate::resolver::learn_own_addresses(&owner, now.iter());
                last = now;
                let Some(core) = me.upgrade() else { break };
                forge_runtime::logging::info!("relay: the network changed; retrying what waits");
                core.retry_now();
            }
            // The endpoint closed: its networks no longer count as local.
            crate::resolver::forget_own_addresses(&owner);
        });
    }

    /// Why `to` wasn't reached the last time Relay tried, in words.
    pub(crate) fn problem(&self, to: &str, reason: String) {
        lock(&self.problems).insert(to.to_string(), reason);
    }

    pub(crate) fn problem_of(&self, to: &str) -> Option<String> {
        lock(&self.problems).get(to).cloned()
    }

    /// Puts one of our conversation messages in `to`'s queue, after the
    /// messages and files already waiting for it, and returns it `Queued`
    /// with what answers its first attempt. Sending a message again (a
    /// retry after it failed) queues it again; one already queued or sent
    /// comes back as it is.
    pub(crate) async fn queue_message(
        &self,
        to: &str,
        message: chat_core::Message,
    ) -> Result<
        (
            chat_core::Message,
            oneshot::Receiver<Result<(), RelayError>>,
        ),
        RelayError,
    > {
        use chat_core::MessageStatus;
        let (tx, rx) = oneshot::channel();
        let trusted = self
            .record
            .get()
            .devices
            .get(to)
            .is_some_and(|d| d.trust == SavedTrust::Trusted);
        if !trusted {
            return Err(RelayError::UnknownPeer { id: to.into() });
        }
        let id = new_id();
        let waiters = Arc::clone(&self.outbox.waiters);
        let recipient = to.to_string();
        // The message, Queued, and its delivery: one transaction (P3).
        let message = self
            .rt
            .command("queue-message", move |state| {
                state.change_with(|r, c| {
                    let message = match c.get(&recipient, &message.id)? {
                        Some(found) if found.status == MessageStatus::Failed => {
                            c.set_status(&recipient, &found.id, MessageStatus::Queued)?;
                            c.get(&recipient, &found.id)?.unwrap_or(found)
                        }
                        Some(found) => {
                            let _ = tx.send(Ok(()));
                            return Ok(found);
                        }
                        None => c.add(
                            &recipient,
                            chat_core::Message {
                                status: MessageStatus::Queued,
                                ..message
                            },
                        )?,
                    };
                    if r.message_delivery(&message.id).is_none() {
                        let payload = Payload::Message {
                            id: message.id.clone(),
                            body: message.body.clone(),
                            reply_to: message.reply_to.clone(),
                        };
                        r.queue_message(&id, &recipient, payload, now_ms());
                        lock(&waiters).entry(hex(&id)).or_default().push(tx);
                    } else {
                        let _ = tx.send(Ok(()));
                    }
                    Ok(message)
                })
            })
            .await?;
        self.wake(to);
        Ok((message, rx))
    }

    /// Sets what wakes device conversations when their device is reached.
    pub(crate) fn set_chat_waker(&self, waker: Option<crate::ChatWaker>) {
        *lock(&self.chat_waker) = waker;
    }

    /// The `ChatRepo` of `endpoint_id`'s conversation.
    pub(crate) fn peer_repo(
        self: &Arc<Self>,
        endpoint_id: &str,
    ) -> Result<crate::device_chat::PeerRepo, RelayError> {
        crate::device_chat::PeerRepo::new(self, endpoint_id)
    }
}

#[doc(hidden)]
impl RelayCore {
    /// Tests only: the next `count` items this device delivers lose the
    /// recipient's answer, as when the connection breaks just after the
    /// recipient saved the item. The sender then tries again.
    pub fn lose_next_answers(&self, count: u32) {
        self.outbox.lose_answers.store(count, Ordering::SeqCst);
    }
}

impl RelayCore {
    /// `d` wasn't delivered directly: whether it now waits in the
    /// recipient's mailbox, already or because it was just left there.
    /// Either way the device isn't tried directly again for now.
    async fn waits_in_mailbox(&self, d: &SavedDelivery) -> bool {
        lock(&self.outbox.direct_again).remove(&d.to);
        let now = now_ms();
        let already = d.in_mailbox(now);
        let transfer = d.transfer;
        let depositing = Arc::clone(&self.outbox.depositing);
        if !already {
            // Decided in a command, so a cancel either ended it first (and
            // it isn't left) or sees the deposit and is refused.
            let (id, marks) = (d.id.clone(), Arc::clone(&depositing));
            let go = self
                .rt
                .command("delivery-depositing", move |state| {
                    if state.record.delivery(&id).is_none() {
                        return Ok::<_, RelayError>(None);
                    }
                    // Already marked: an earlier deposit whose `left_ms`
                    // couldn't be saved, so it may be in the mailbox.
                    Ok(Some(!lock(&marks).insert(transfer)))
                })
                .await;
            let Ok(Some(earlier)) = go else {
                return false;
            };
            if let Err(why) = self.leave_in_mailbox(d).await {
                if !earlier {
                    lock(&depositing).remove(&transfer);
                }
                forge_runtime::logging::info!("relay: not left in the mailbox: {why}");
                return false;
            }
        }
        let id = d.id.clone();
        let to = d.to.clone();
        // A message shows "In the Mailbox" in its conversation.
        let message = match &d.payload {
            Payload::Message { id, .. } => Some(id.clone()),
            Payload::Files { .. } => Some(crate::device_chat::files_message_id(&d.id)),
            Payload::Note { .. } => None,
        };
        let saved = self
            .rt
            .command("delivery-left", move |state| {
                state.change_with(|r, c| {
                    r.delivery_status(&id, SavedStatus::Waiting);
                    let left = already || r.left_in_mailbox(&id, now);
                    if let (true, Some(message)) = (left, message) {
                        c.set_in_mailbox(&to, &message)?;
                    }
                    Ok(left)
                })
            })
            .await;
        // Kept when that couldn't be saved: the item is in the mailbox all
        // the same, so it stays out of reach of a cancel until a relaunch.
        if saved.is_ok() {
            lock(&depositing).remove(&transfer);
        }
        matches!(saved, Ok(true))
    }

    /// Whether the send for history entry `transfer` is out of this
    /// device's hands: sealed in the recipient's mailbox, or being left
    /// there now. Such a send can't be cancelled.
    pub(crate) fn out_of_hands(&self, transfer: u64) -> bool {
        lock(&self.outbox.depositing).contains(&transfer)
            || self.record.get().sent_to_mailbox(transfer, now_ms())
    }

    /// A receipt from `from` for the delivery `id`: it has the item, so the
    /// delivery is Done, as if it had answered directly. False when that
    /// couldn't be saved now (the receipt is kept for the next round).
    pub(crate) async fn receipt(&self, from: &str, id: &str) -> bool {
        let Some(d) = self
            .record
            .get()
            .delivery(id)
            .filter(|d| d.to == from)
            .cloned()
        else {
            // Ended already, or not one of ours: nothing to do.
            return true;
        };
        let (id, d_for_end) = (d.id.clone(), d.clone());
        let ended = self
            .rt
            .command("delivery-receipt", move |state| {
                state.change_with(|r, c| {
                    let ended = r.end_delivery(&id, SavedStatus::Done);
                    if ended {
                        message_ended(c, &d_for_end, true)?;
                    }
                    Ok(ended)
                })
            })
            .await;
        match ended {
            Ok(true) => {
                self.let_go(&d).await;
                true
            }
            Ok(false) => true,
            Err(e) => {
                forge_runtime::logging::warn!("relay: a receipt wasn't saved: {e}");
                false
            }
        }
    }
}

/// `to`'s worker: its items oldest first, until none is due.
async fn drain(me: Weak<RelayCore>, to: String) {
    loop {
        let Some(core) = me.upgrade() else { return };
        let next = {
            let mut draining = lock(&core.outbox.draining);
            match core.due(&to) {
                Some(d) => d,
                None => {
                    draining.remove(&to);
                    drop(draining);
                    core.settle(&to);
                    return;
                }
            }
        };
        core.attempt(next).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str) -> (SavedItem, Payload) {
        (
            SavedItem::Note { text: text.into() },
            Payload::Note { text: text.into() },
        )
    }

    #[test]
    fn items_wait_in_order_per_recipient() {
        let mut r = Record::default();
        for (i, (to, text)) in [("a", "1"), ("b", "x"), ("a", "2")].iter().enumerate() {
            let (item, payload) = note(text);
            r.queue(&[i as u8; 16], to, item, payload, 10);
        }
        assert_eq!(r.waiting_for(), vec!["a".to_string(), "b".to_string()]);
        let first = r.next_for("a").unwrap().clone();
        assert_eq!(first.payload, Payload::Note { text: "1".into() });
        assert_eq!(r.transfers[0].status, SavedStatus::Waiting);
        assert!(r.end_delivery(&first.id, SavedStatus::Done));
        assert!(!r.end_delivery(&first.id, SavedStatus::Done), "ends once");
        assert_eq!(r.transfers[0].status, SavedStatus::Done);
        assert_eq!(
            r.next_for("a").unwrap().payload,
            Payload::Note { text: "2".into() }
        );
    }

    #[test]
    fn an_item_left_in_the_mailbox_is_due_again_when_it_expires() {
        use crate::mailbox_server::KEPT_MS;
        let mut r = Record::default();
        let (item, payload) = note("1");
        r.queue(&[1; 16], "a", item, payload, 10);
        let id = r.next_for("a").unwrap().id.clone();
        assert!(r.left_expired(100).is_empty(), "never left");
        assert!(r.left_in_mailbox(&id, 100));
        assert!(
            r.next_due("a", false, 101).is_none(),
            "waits in the mailbox"
        );
        assert!(
            r.next_due("a", true, 101).is_some(),
            "tried directly when seen"
        );
        assert!(r.left_expired(100 + KEPT_MS - 1).is_empty());
        assert_eq!(r.left_expired(100 + KEPT_MS), vec!["a".to_string()]);
        assert!(r.next_due("a", false, 100 + KEPT_MS).is_some());
    }

    #[test]
    fn a_relaunch_leaves_pending_items_waiting() {
        let mut r = Record::default();
        let (item, payload) = note("1");
        let t = r.queue(&[1; 16], "a", item, payload, 10);
        let id = r.deliveries[0].id.clone();
        r.delivery_status(&id, SavedStatus::InProgress);
        r.interrupted();
        let entry = r.transfers.iter().find(|x| x.id == t).unwrap();
        assert_eq!(entry.status, SavedStatus::Waiting);
        assert_eq!(r.deliveries.len(), 1);
    }

    #[test]
    fn received_ids_are_kept_45_days() {
        let mut r = Record::default();
        r.remember_received("a", &[1; 16], 0);
        r.remember_received("a", &[1; 16], 5);
        assert_eq!(r.received_ids.len(), 1);
        assert!(r.received_before("a", &[1; 16]));
        assert!(!r.received_before("b", &[1; 16]), "per sender");
        r.remember_received("a", &[2; 16], RECEIVED_KEPT_MS + 1);
        assert!(!r.received_before("a", &[1; 16]));
        assert!(r.received_before("a", &[2; 16]));
    }

    #[test]
    fn backoff_doubles_to_five_minutes() {
        let mut b = backoff();
        let within =
            |d: Duration, ms: u64| d.as_millis() as u64 <= ms && d.as_millis() as u64 >= ms * 4 / 5;
        assert!(within(b.next_delay(), 1_000));
        assert!(within(b.next_delay(), 2_000));
        for _ in 0..20 {
            b.next_delay();
        }
        assert!(within(b.next_delay(), 300_000));
        const { assert!(PENDING_KEPT_MS < RECEIVED_KEPT_MS) };
    }

    #[test]
    fn a_files_payload_round_trips_to_an_offer() {
        let hash = Hash::from_bytes([3; 32]);
        let offer = Offer {
            hash,
            collection: true,
            size: 5,
            name: "Trip".into(),
            hashes: HashSet::from([hash, Hash::from_bytes([4; 32])]),
        };
        let payload = Payload::files(&offer);
        let back = payload.offer().unwrap();
        assert_eq!(back.hash, hash);
        assert_eq!(back.hashes, offer.hashes);
        assert_eq!(payload.content(), Some(HashAndFormat::hash_seq(hash)));
    }
}
