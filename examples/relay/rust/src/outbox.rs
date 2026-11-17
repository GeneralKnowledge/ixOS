//! The delivery outbox (Relay M1 stage 3,
//! examples/relay/docs/relay-reliable-delivery.md and
//! relay-delivery-outbox.md): notes and files for a trusted device wait
//! here until it is reachable, survive a relaunch, and arrive once.
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
//! - **Two state machines.** A note is Waiting, then Sending, then
//!   Delivered. A file is Waiting, then Sending while the recipient
//!   fetches what it doesn't have yet (so a retry resumes), then Delivered
//!   once the recipient has verified and saved it. Either ends Failed with
//!   a cause: refused or declined by the recipient, no longer trusted here,
//!   cancelled, or not delivered within 30 days.
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
use crate::model::{RelayError, Transfer};
use crate::record::{Record, SavedItem, SavedStatus};
use crate::trust::SavedTrust;
use crate::wire::{hex, unhex};
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
    /// Its history entry.
    pub transfer: u64,
    pub payload: Payload,
    pub queued_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Payload {
    Note {
        text: String,
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
            Payload::Note { .. } => None,
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
    forge_devproto::new_secret()[..16]
        .try_into()
        .expect("16 bytes")
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
        });
        transfer
    }

    /// The oldest item for `to`: the next to send.
    pub fn next_for(&self, to: &str) -> Option<&SavedDelivery> {
        self.deliveries.iter().find(|d| d.to == to)
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
    /// Sends waiting for their item's first attempt, by history id.
    pub(crate) waiters: Arc<Mutex<HashMap<u64, Vec<Waiter>>>>,
    /// Tests: the next answers to lose, as if the connection broke after
    /// the recipient saved the item.
    lose_answers: AtomicU32,
}

/// How one attempt went.
enum Outcome {
    Delivered(Vec<String>),
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
        let (tx, rx) = oneshot::channel();
        let waiters = Arc::clone(&self.outbox.waiters);
        let recipient = to.clone();
        let transfer = self
            .rt
            .command("queue", move |state| {
                state.change(|r| {
                    let transfer = r.queue(&id, &recipient, item, payload, now_ms());
                    // Before the change is published, so the worker can't
                    // finish the item before anyone waits for it.
                    lock(&waiters).entry(transfer).or_default().push(tx);
                    Ok(transfer)
                })
            })
            .await?;
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
        self.record.get().next_for(to).cloned()
    }

    /// Answers the sends still waiting on `to`'s items: they wait.
    fn settle(&self, to: &str) {
        let record = self.record.get();
        let mut waiters = lock(&self.outbox.waiters);
        for d in record.deliveries.iter().filter(|d| d.to == to) {
            for tx in waiters.remove(&d.transfer).unwrap_or_default() {
                let _ = tx.send(Ok(()));
            }
        }
    }

    fn answer(&self, transfer: u64, result: Result<(), RelayError>) {
        for tx in lock(&self.outbox.waiters)
            .remove(&transfer)
            .unwrap_or_default()
        {
            let _ = tx.send(result.clone());
        }
    }

    /// One attempt at `d`; returns false when the device wasn't reached.
    async fn attempt(&self, d: SavedDelivery) -> bool {
        let outcome = self.try_once(&d).await;
        let reached = !matches!(outcome, Outcome::Retry);
        let (status, answer, seen) = match outcome {
            Outcome::Gone => return true,
            Outcome::Delivered(addresses) => (SavedStatus::Done, Ok(()), Some(addresses)),
            Outcome::Ended(reason, err) => (SavedStatus::Failed { reason }, Err(err), None),
            Outcome::Retry => (SavedStatus::Waiting, Ok(()), None),
        };
        let (id, to) = (d.id.clone(), d.to.clone());
        let saved = self
            .rt
            .command("delivery", move |state| {
                state.change(|r| {
                    if let Some(addresses) = seen {
                        r.devices.seen_at(&to, &addresses);
                    }
                    Ok(if status == SavedStatus::Waiting {
                        r.delivery_status(&id, status);
                        false
                    } else {
                        r.end_delivery(&id, status)
                    })
                })
            })
            .await;
        if let Err(err) = &saved {
            forge_runtime::logging::warn!("relay couldn't save a delivery: {err}");
        }
        if matches!(saved, Ok(true)) {
            self.let_go(&d).await;
        }
        self.answer(d.transfer, answer);
        let mut backoff = lock(&self.outbox.backoff);
        if reached {
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
        let addresses = lock(&self.nearby)
            .get(&d.to)
            .cloned()
            .filter(|a| !a.is_empty())
            .unwrap_or(saved.addresses);
        let Ok(addr) = device::endpoint_addr(&d.to, &addresses) else {
            return Outcome::Retry;
        };
        let outgoing = match &d.payload {
            Payload::Note { text } => device::Outgoing::Note(text.clone()),
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
        lock(&self.device_stops).insert(transfer, Arc::clone(&stop));
        let node = Arc::clone(&self.node);
        let my_name = self.identity().name;
        let weak = self.me.clone();
        let result = code::TOKIO
            .spawn(async move {
                let node = node.get().await?;
                let report = |done: u64| {
                    if let Some(core) = weak.upgrade() {
                        lock(&core.progress).insert(transfer, done);
                    }
                };
                device::send(&node, addr, my_name, id, outgoing, report, stop).await
            })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)));
        lock(&self.device_stops).remove(&transfer);
        lock(&self.progress).remove(&transfer);
        let result = match result {
            Ok(_) if self.lose_answer() => Err(code::connection("the answer was lost")),
            other => other,
        };
        match result {
            Ok(_) => Outcome::Delivered(addresses),
            Err(RelayError::Unreachable { .. } | RelayError::Connection { .. }) => Outcome::Retry,
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
    /// ends Cancelled. False when `transfer` isn't pending.
    pub(crate) async fn cancel_delivery(&self, transfer: u64) -> bool {
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
        let id = d.id.clone();
        let ended = self
            .rt
            .command("cancel-delivery", move |state| {
                state.change(|r| {
                    Ok(r.end_delivery(
                        &id,
                        SavedStatus::Failed {
                            reason: "Cancelled".into(),
                        },
                    ))
                })
            })
            .await;
        if matches!(ended, Ok(true)) {
            self.let_go(&d).await;
            self.answer(
                transfer,
                Err(RelayError::Stopped {
                    kind: forge_app::ErrorKind::Cancelled,
                }),
            );
        }
        true
    }
}

impl RelayCore {
    /// Sends one conversation message to a trusted device now, by its
    /// own id (`device_chat`): the caller's outbox keeps it while the
    /// device can't be reached.
    pub(crate) async fn deliver_message(
        &self,
        to: &str,
        message: crate::device_chat::PeerMessage,
    ) -> Result<(), RelayError> {
        let saved = self.record.get().devices.get(to).cloned();
        let Some(saved) = saved.filter(|s| s.trust == SavedTrust::Trusted) else {
            return Err(RelayError::UnknownPeer { id: to.into() });
        };
        let addresses = lock(&self.nearby)
            .get(to)
            .cloned()
            .filter(|a| !a.is_empty())
            .unwrap_or(saved.addresses);
        let addr = device::endpoint_addr(to, &addresses)?;
        let node = Arc::clone(&self.node);
        let my_name = self.identity().name;
        let stop = Arc::new(AtomicBool::new(false));
        code::TOKIO
            .spawn(async move {
                let node = node.get().await?;
                let outgoing = device::Outgoing::Message(message);
                device::send(&node, addr, my_name, new_id(), outgoing, |_| {}, stop).await
            })
            .await
            .unwrap_or_else(|e| Err(code::connection(e)))?;
        if self.lose_answer() {
            return Err(code::connection("the answer was lost"));
        }
        let to = to.to_string();
        let _ = self
            .rt
            .command("message-reached", move |state| {
                state.change(|r| {
                    r.devices.seen_at(&to, &addresses);
                    Ok(())
                })
            })
            .await;
        Ok(())
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
