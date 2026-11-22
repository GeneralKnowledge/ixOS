//! The mailbox client (N3 step 3, part 2; examples/relay/docs/relay-mailbox.md):
//! leaving an item in a trusted device's mailbox when it can't be reached,
//! and collecting what waits in this device's own.
//!
//! - **Leaving.** When a message or note isn't delivered directly and the
//!   recipient told this device its mailbox (in the Hello), the outbox seals
//!   it to the recipient's mailbox key and leaves it there under the
//!   recipient's pass. It then waits for a receipt; the next item goes on.
//!   Files still wait for a direct route (a later step).
//! - **Collecting** runs once when the node starts, every 30 seconds while
//!   Relay runs, and when Relay comes to the foreground. Each round
//!   registers the passes of the devices trusted here (and deregisters the
//!   revoked and forgotten ones'), then for each item: opens it (only from
//!   a device trusted here), saves it once (by the same ids a direct
//!   delivery uses), leaves a receipt in the sender's mailbox, and only then
//!   acknowledges it, so the server deletes it. A receipt ends the
//!   matching delivery here as Done. An item this device won't take is
//!   deleted with no receipt. An item the mailbox let expire (7 days) is
//!   tried directly again, then left again.
//! - Only in Hybrid, and only while Relay runs, like the outbox.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use forge_runtime::now_ms;
use iroh::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};

use crate::hex::{hex, unhex};
use crate::mailbox::{open, seal, DepositPass, MailboxKey, PeerMailbox};
use crate::mailbox_server::{CallError, Kind, MailboxClient, KEPT_MS};
use crate::node::Node;
use crate::outbox::{Payload, SavedDelivery};
use crate::trust::SavedTrust;
use crate::{ConnectionMode, RelayCore};

/// How often the collector looks while Relay runs.
const EVERY: Duration = Duration::from_secs(30);

/// What a sealed item holds: the outbox's item. A new variant is a new
/// kind of item; an older build drops it (and still acknowledges it).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) enum Posted {
    Note {
        text: String,
    },
    Message {
        id: String,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
    },
}

impl Posted {
    /// What of `payload` the mailbox takes: messages and notes.
    pub(crate) fn of(payload: &Payload) -> Option<Self> {
        match payload {
            Payload::Note { text } => Some(Posted::Note { text: text.clone() }),
            Payload::Message { id, body, reply_to } => Some(Posted::Message {
                id: id.clone(),
                body: body.clone(),
                reply_to: reply_to.clone(),
            }),
            Payload::Files { .. } => None,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        ciborium::into_writer(self, &mut out).expect("CBOR into memory");
        out
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        ciborium::from_reader(bytes).ok()
    }
}

/// A receipt's payload: nothing beyond its header's id.
const RECEIPT: &[u8] = b"";

fn id16(text: &str) -> Option<[u8; 16]> {
    unhex(text)?.try_into().ok()
}

/// Seals `payload` for the device `to` and leaves it in its mailbox, as
/// `kind`. True when the mailbox already had it.
async fn leave(
    node: &Node,
    to: &PublicKey,
    mailbox: &PeerMailbox,
    kind: Kind,
    id: [u8; 16],
    payload: &[u8],
) -> Result<bool, String> {
    let (Some(key), Some(pass), Some(server)) = (mailbox.key(), mailbox.pass(), mailbox.server())
    else {
        return Err("its mailbox, as kept here, is damaged".into());
    };
    let item = seal(
        node.endpoint.secret_key(),
        to,
        &key,
        id,
        now_ms() + KEPT_MS,
        payload,
    )
    .map_err(|e| e.to_string())?;
    let client = MailboxClient::connect(&node.endpoint, node.mailbox_addr(server))
        .await
        .map_err(|e| e.to_string())?;
    let left = client.deposit(&pass, kind, &item).await;
    client.close();
    left.map_err(|e| e.to_string())
}

/// The pass digests this device registers: one per device trusted here;
/// and the ones it takes back: revoked and forgotten devices'.
fn passes(core: &RelayCore, me: &SecretKey) -> (Vec<[u8; 32]>, Vec<[u8; 32]>) {
    let (mut keep, mut gone) = (Vec::new(), Vec::new());
    for d in &core.record.get().devices.list {
        let Ok(peer) = d.endpoint_id.parse::<PublicKey>() else {
            continue;
        };
        let digest = DepositPass::for_peer(me, &peer).digest();
        if d.trust == SavedTrust::Trusted {
            keep.push(digest);
        } else {
            gone.push(digest);
        }
    }
    (keep, gone)
}

impl RelayCore {
    /// Leaves `d` in its recipient's mailbox, if the recipient told one and
    /// the mailbox takes this kind of item.
    pub(crate) async fn leave_in_mailbox(&self, d: &SavedDelivery) -> Result<(), String> {
        if self.connection_mode() != ConnectionMode::Hybrid {
            return Err("only Hybrid uses the mailbox".into());
        }
        let posted = Posted::of(&d.payload).ok_or("files wait for a direct route")?;
        let mailbox = self
            .record
            .get()
            .devices
            .get(&d.to)
            .filter(|s| s.trust == SavedTrust::Trusted)
            .and_then(|s| s.mailbox.clone())
            .ok_or("it hasn't told this device a mailbox")?;
        let to: PublicKey = d.to.parse().map_err(|_| "not a device id")?;
        let id = id16(&d.id).ok_or("a damaged delivery id")?;
        let node = self.node.started().ok_or("Relay isn't open to devices")?;
        let payload = posted.encode();
        crate::code::TOKIO
            .spawn(async move { leave(&node, &to, &mailbox, Kind::Item, id, &payload).await })
            .await
            .map_err(|e| e.to_string())??;
        forge_runtime::logging::info!("relay: an item was left in a device's mailbox");
        Ok(())
    }

    /// Starts collecting from this device's mailbox: now, every 30 s, and
    /// whenever [`collect_mailbox`](Self::collect_mailbox) asks. Once.
    pub(crate) fn start_collector(&self) {
        if self.outbox.collector_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.me.clone();
        crate::code::TOKIO.spawn(async move {
            loop {
                let Some(core) = me.upgrade() else { return };
                if let Err(why) = core.collect_once().await {
                    forge_runtime::logging::info!("relay: the mailbox wasn't collected: {why}");
                }
                // Waits without holding the core, so Relay can close.
                let wake = Arc::clone(&core.outbox.collect_now);
                drop(core);
                let _ = tokio::time::timeout(EVERY, wake.notified()).await;
            }
        });
    }

    /// Collects from this device's mailbox now, without trying any
    /// device directly (Relay came to the foreground, or a device met).
    pub fn collect_mailbox(&self) {
        self.outbox.collect_now.notify_one();
    }

    /// One round: register passes, then open, save, confirm and
    /// acknowledge everything waiting.
    async fn collect_once(&self) -> Result<(), String> {
        // Items the mailbox has let expire are due again: their devices'
        // workers try them directly (keeping any backoff), and leave them
        // once more if that fails.
        for to in self.record.get().left_expired(now_ms()) {
            self.wake(&to);
        }
        if self.connection_mode() != ConnectionMode::Hybrid {
            return Ok(());
        }
        let Some(node) = self.node.started() else {
            return Ok(());
        };
        let Some(server) = node.mailbox_server() else {
            return Ok(());
        };
        let me = node.endpoint.secret_key().clone();
        let (keep, gone) = passes(self, &me);
        let client = MailboxClient::connect(&node.endpoint, node.mailbox_addr(server))
            .await
            .map_err(|e| e.to_string())?;
        let result = self.collect_with(&node, &client, &me, keep, gone).await;
        client.close();
        result.map_err(|e| e.to_string())
    }

    async fn collect_with(
        &self,
        node: &Arc<Node>,
        client: &MailboxClient,
        me: &SecretKey,
        keep: Vec<[u8; 32]>,
        gone: Vec<[u8; 32]>,
    ) -> Result<(), CallError> {
        if !gone.is_empty() {
            client.deregister(gone).await?;
        }
        if !keep.is_empty() {
            log::debug!(
                "mailbox client: {} registering {} passes",
                me.public().fmt_short(),
                keep.len()
            );
            client.register(keep).await?;
        }
        let key = MailboxKey::of_device(me);
        let listed = client.list().await?;
        log::debug!(
            "mailbox client: {} has {} waiting",
            me.public().fmt_short(),
            listed.len()
        );
        for waiting in listed {
            let (kind, item) = match client.fetch(waiting.from, waiting.id).await {
                Ok(fetched) => fetched,
                Err(CallError::Refused(r)) => {
                    forge_runtime::logging::info!("relay: a mailbox item couldn't be fetched: {r}");
                    continue;
                }
                Err(e) => return Err(e),
            };
            let record = self.record.get();
            let opened = open(&key, &me.public(), &item, now_ms(), |from| {
                record
                    .devices
                    .get(&from.to_string())
                    .is_some_and(|d| d.trust == SavedTrust::Trusted)
            });
            let opened = match opened {
                Ok(opened) => opened,
                Err(why) => {
                    // Never readable later either: let it go.
                    forge_runtime::logging::info!("relay: a mailbox item was dropped: {why}");
                    client.ack(waiting.from, waiting.id).await?;
                    continue;
                }
            };
            let from = opened.from.to_string();
            match kind {
                Kind::Receipt => {
                    if !self.receipt(&from, &hex(&opened.id)).await {
                        // Not saved: it stays, and is read again next round.
                        continue;
                    }
                }
                Kind::Item => match self.take_posted(&from, opened.id, &opened.payload).await {
                    // Not saved: it stays, and is tried next round.
                    Taken::Retry => continue,
                    // Not taken: no receipt, so the sender's delivery
                    // doesn't end Done.
                    Taken::Dropped => {}
                    Taken::Saved => {
                        // With no mailbox known for the sender there is no
                        // receipt: its delivery ends when the two next meet
                        // directly, and that retry is answered without saving
                        // the item again (the same ids a direct delivery uses).
                        let sender = record.devices.get(&from).and_then(|d| d.mailbox.clone());
                        if let Some(sender) = sender {
                            if let Err(why) = leave(
                                node,
                                &opened.from,
                                &sender,
                                Kind::Receipt,
                                opened.id,
                                RECEIPT,
                            )
                            .await
                            {
                                // The receipt goes before the item is deleted:
                                // both are tried again next round.
                                forge_runtime::logging::info!("relay: no receipt left: {why}");
                                continue;
                            }
                        }
                    }
                },
            }
            client.ack(waiting.from, waiting.id).await?;
        }
        Ok(())
    }

    /// Saves an item from `from` once, as a direct delivery would.
    async fn take_posted(&self, from: &str, id: [u8; 16], payload: &[u8]) -> Taken {
        use crate::device::DeviceInbound as _;
        let Some(posted) = Posted::decode(payload) else {
            forge_runtime::logging::info!(
                "relay: a mailbox item this build can't read was dropped"
            );
            return Taken::Dropped;
        };
        let host = crate::CoreDevices {
            core: self.me.clone(),
        };
        if !host.still_takes_from(from) {
            forge_runtime::logging::info!(
                "relay: a mailbox item from a device not taken from was dropped"
            );
            return Taken::Dropped;
        }
        let from = from.to_string();
        let saved = tokio::task::spawn_blocking(move || match posted {
            Posted::Note { text } => {
                if host.received_before(&from, &id) {
                    return Ok(());
                }
                host.note(&from, id, text)
            }
            Posted::Message { id, body, reply_to } => host.message(
                &from,
                crate::device_chat::PeerMessage { id, body, reply_to },
            ),
        })
        .await;
        match saved {
            Ok(Ok(())) => Taken::Saved,
            Ok(Err(e)) => {
                forge_runtime::logging::warn!("relay: a mailbox item wasn't saved: {e}");
                Taken::Retry
            }
            Err(_) => Taken::Retry,
        }
    }
}

/// What became of an item collected from the mailbox.
enum Taken {
    /// Saved now, or before: a receipt goes back.
    Saved,
    /// One this device won't take: deleted with no receipt.
    Dropped,
    /// Not saved now: it stays for the next round.
    Retry,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload's CBOR is what devices of different builds exchange.
    #[test]
    fn posted_items_round_trip() {
        for posted in [
            Posted::Note { text: "hi".into() },
            Posted::Message {
                id: "m1".into(),
                body: "hello".into(),
                reply_to: Some("m0".into()),
            },
        ] {
            assert_eq!(Posted::decode(&posted.encode()), Some(posted));
        }
        assert_eq!(Posted::decode(b"\xff"), None);
        let files = Payload::Files {
            hash: String::new(),
            collection: false,
            name: "a".into(),
            size: 1,
            hashes: vec![],
        };
        assert_eq!(Posted::of(&files), None);
    }
}
