//! Group chats (N7, first part; examples/relay/docs/relay-groups.md).
//!
//! A group is a record every member keeps (`Record::groups`, the `groups`
//! table): its id, name, admin and members, raised by one `version` on
//! every change. Only the admin changes it, by sending the whole record
//! (`GroupUpdate`) to every member, old and new; a member keeps the
//! highest version it was sent by the admin. Any member can leave
//! (`GroupLeave`).
//!
//! A message to a group goes once to each other member, on
//! `relay/device/1`, through Relay's outbox: the same per-recipient queue,
//! retries, exactly-once and mailbox as a message to one device. So each
//! copy is encrypted end to end by the connection it travels on (or sealed
//! to the member's mailbox key), there is no host, and a removed member
//! simply isn't sent to again. No group key, no new cryptography.
//!
//! The conversation is `group:<id>` on chat-core's engine, its messages
//! rows of Relay's database like a device conversation's
//! ([`crate::device_chat`]). Which members have a message is kept per
//! member (`group_delivered`), for "Delivered to 2 of 3".
//!
//! In this first step every member a message goes to must be a trusted
//! device of the sender; members it hasn't paired with are skipped (and
//! shown so). Members introduced by the admin come next.

use std::sync::{Arc, Weak};

use chat_core::{
    ChatError, ChatRepo, Message, MessageStatus, OutgoingMessage, RemoteConversation, RemoteStream,
};
use forge_net::CancelHandle;
use forge_runtime::{lock, now_ms};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::device_chat::{chat_error, Conversations, PeerLog, PeerMessage, ME};
use crate::hex::{hex, unhex};
use crate::model::RelayError;
use crate::outbox::{new_id, Payload};
use crate::record::Record;
use crate::trust::SavedTrust;
use crate::RelayCore;

/// The most members a group has, the admin included, while each message
/// goes once to each member.
pub const MAX_MEMBERS: usize = 32;
const PREFIX: &str = "group:";

/// The conversation id of group `id` (32 hex characters).
pub fn group_conversation(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// The conversation id of group `group_id`, for the app.
#[uniffi::export]
pub fn group_conversation_id(group_id: String) -> String {
    group_conversation(&group_id)
}

/// The group a conversation id names, if it names one.
pub fn conversation_group(conversation_id: &str) -> Option<&str> {
    conversation_id
        .strip_prefix(PREFIX)
        .filter(|id| is_group_id(id))
}

/// Whether `id` can be a group's id: 16 bytes in lowercase hex.
pub(crate) fn is_group_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

fn is_endpoint_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// One member of a group: its device's key, and the name the admin knows
/// it by.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GroupMember {
    pub endpoint_id: String,
    pub name: String,
}

/// A group as this device keeps it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SavedGroup {
    /// 16 random bytes, hex, chosen by the admin.
    pub id: String,
    pub name: String,
    /// The endpoint id of the device that made it: the only one that
    /// changes it.
    pub admin: String,
    /// Everyone in it, the admin included, in the order added.
    pub members: Vec<GroupMember>,
    /// Raised by one on every change by the admin.
    pub version: u64,
    /// This device left, or was removed: the conversation stays, read-only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub left: bool,
}

impl SavedGroup {
    pub fn member(&self, endpoint_id: &str) -> Option<&GroupMember> {
        self.members.iter().find(|m| m.endpoint_id == endpoint_id)
    }

    /// The members other than `me`.
    pub fn others<'a>(&'a self, me: &'a str) -> impl Iterator<Item = &'a GroupMember> + 'a {
        self.members.iter().filter(move |m| m.endpoint_id != me)
    }

    /// The group as it goes to members: without this device's own `left`.
    fn shared(&self) -> SavedGroup {
        SavedGroup {
            left: false,
            ..self.clone()
        }
    }

    /// Checks a group that came from another device, and makes its names
    /// safe to show.
    fn checked(mut self) -> Result<Self, RelayError> {
        let bad = |why: &str| RelayError::Declined {
            reason: format!("A group update that can't be used: {why}"),
        };
        if !is_group_id(&self.id) {
            return Err(bad("its id"));
        }
        if self.members.is_empty() || self.members.len() > MAX_MEMBERS {
            return Err(bad("its number of members"));
        }
        let mut seen = std::collections::HashSet::new();
        for m in &mut self.members {
            if !is_endpoint_id(&m.endpoint_id) || !seen.insert(m.endpoint_id.clone()) {
                return Err(bad("a member"));
            }
            m.name = crate::device::clean_offered_name(&m.name, "Unnamed device");
        }
        if self.member(&self.admin).is_none() {
            return Err(bad("its admin isn't a member"));
        }
        self.name = crate::device::clean_offered_name(&self.name, "Group");
        self.left = false;
        Ok(self)
    }
}

/// What a group item from another device says, on the wire or from the
/// mailbox.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GroupItem {
    Message { group: String, message: PeerMessage },
    Update(SavedGroup),
    Leave { group: String },
}

impl Record {
    pub fn group(&self, id: &str) -> Option<&SavedGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: &str) -> Option<&mut SavedGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Takes a group item from `from`; `me` is this device's endpoint id.
    /// An item that isn't allowed is refused, in words for the sender.
    pub(crate) fn take_group_item(
        &mut self,
        c: &mut Conversations<'_>,
        me: &str,
        from: &str,
        item: GroupItem,
    ) -> Result<(), RelayError> {
        let refuse = |reason: &str| {
            Err(RelayError::Declined {
                reason: reason.into(),
            })
        };
        match item {
            GroupItem::Message { group, message } => {
                let Some(g) = self.group(&group) else {
                    return refuse("This device isn't in that group");
                };
                if g.left {
                    return refuse("This device has left the group");
                }
                let Some(member) = g.member(from) else {
                    return refuse("You aren't a member of that group here");
                };
                if message.id.is_empty()
                    || message.id.chars().count() > crate::device_chat::MAX_ID_CHARS
                {
                    return refuse("A message id that isn't usable");
                }
                let body = crate::model::clean_note(&message.body)?;
                // A trusted device's own name here wins over the admin's.
                let author = self
                    .devices
                    .get(from)
                    .filter(|d| d.trust == SavedTrust::Trusted)
                    .map(|d| d.shown_name().to_string())
                    .unwrap_or_else(|| member.name.clone());
                c.add(
                    &group_conversation(&group),
                    Message {
                        id: message.id,
                        author,
                        author_id: from.to_string(),
                        body,
                        reply_to: message.reply_to,
                        ..Message::default()
                    },
                )?;
                Ok(())
            }
            GroupItem::Update(update) => {
                let update = update.checked()?;
                if from != update.admin {
                    return refuse("Only the group's admin changes it");
                }
                let mine = update.member(me).is_some();
                match self.group_mut(&update.id) {
                    Some(g) => {
                        if g.admin != from {
                            return refuse("Only the group's admin changes it");
                        }
                        if update.version <= g.version {
                            // Already here, or older: a retry.
                            return Ok(());
                        }
                        *g = SavedGroup {
                            left: !mine,
                            ..update
                        };
                    }
                    // Not added, or already removed before it arrived:
                    // nothing to keep.
                    None if !mine => {}
                    None => self.groups.push(update),
                }
                Ok(())
            }
            GroupItem::Leave { group } => {
                if let Some(g) = self.group_mut(&group) {
                    if from != me {
                        g.members.retain(|m| m.endpoint_id != from);
                    }
                }
                Ok(())
            }
        }
    }

    /// Pending deliveries of group message `id` in group `group`.
    fn pending_group_message(&self, group: &str, id: &str) -> usize {
        self.deliveries
            .iter()
            .filter(|d| matches!(&d.payload, Payload::GroupMessage { group: g, id: m, .. } if g == group && m == id))
            .count()
    }
}

/// A group delivery ended: one member has the message (or won't). Once no
/// copy of it waits, the message shows `Sent` if any member has it, or
/// `Failed` (sending it again tries the rest). In the transaction that
/// ends the delivery, after it left the record.
pub(crate) fn group_delivery_ended(
    r: &Record,
    c: &mut Conversations<'_>,
    group: &str,
    id: &str,
    to: &str,
    delivered: bool,
) -> Result<(), RelayError> {
    let conversation = group_conversation(group);
    if delivered {
        crate::store::set_group_delivered(c.connection(), &conversation, id, to)?;
    }
    if r.pending_group_message(group, id) > 0 {
        return Ok(());
    }
    let any = !crate::store::group_delivered(c.connection(), &conversation, id)?.is_empty();
    let status = if any {
        MessageStatus::Sent
    } else {
        MessageStatus::Failed
    };
    c.set_status(&conversation, id, status)
}

/// At open: a group message shown queued that no delivery holds failed,
/// as [`crate::outbox::settle_messages`] does for devices.
pub(crate) fn settle_group_messages(r: &Record, c: &mut Conversations<'_>) {
    for g in &r.groups {
        let conversation = group_conversation(&g.id);
        for m in c.queued(&conversation).unwrap_or_default() {
            if r.pending_group_message(&g.id, &m.id) == 0 {
                let _ = c.set_status(&conversation, &m.id, MessageStatus::Failed);
            }
        }
    }
}

/// A group, for the app and the `relay` command.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RelayGroup {
    pub id: String,
    /// Its conversation in Chats.
    pub conversation_id: String,
    pub name: String,
    pub admin: String,
    /// Whether this device is the admin: it may add, remove and rename.
    pub is_admin: bool,
    pub members: Vec<RelayGroupMember>,
    /// This device left or was removed: the conversation is read-only.
    pub left: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RelayGroupMember {
    pub endpoint_id: String,
    pub name: String,
    /// This device.
    pub is_me: bool,
    /// A trusted device here: messages go to it. A member this device
    /// hasn't paired with is skipped for now.
    pub trusted: bool,
}

/// How far one of our group messages has got.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GroupDelivery {
    /// Members that have it.
    pub delivered: u32,
    /// Members it was for (everyone but this device).
    pub of: u32,
}

/// What waits on the first attempt of each copy of a group item.
type Firsts = Vec<oneshot::Receiver<Result<(), RelayError>>>;

impl RelayCore {
    /// This device's endpoint id.
    async fn me(&self) -> Result<String, RelayError> {
        Ok(self.node().await?.id().to_string())
    }

    fn group_info(&self, g: &SavedGroup, me: &str) -> RelayGroup {
        let record = self.record.get();
        RelayGroup {
            id: g.id.clone(),
            conversation_id: group_conversation(&g.id),
            name: g.name.clone(),
            admin: g.admin.clone(),
            is_admin: g.admin == me,
            members: g
                .members
                .iter()
                .map(|m| {
                    let is_me = m.endpoint_id == me;
                    let device = record
                        .devices
                        .get(&m.endpoint_id)
                        .filter(|d| d.trust == SavedTrust::Trusted);
                    RelayGroupMember {
                        endpoint_id: m.endpoint_id.clone(),
                        name: if is_me {
                            record.name.clone()
                        } else {
                            device
                                .map(|d| d.shown_name().to_string())
                                .unwrap_or_else(|| m.name.clone())
                        },
                        is_me,
                        trusted: device.is_some(),
                    }
                })
                .collect(),
            left: g.left,
        }
    }
}

/// Group chats for the app (step 3).
#[uniffi::export]
impl RelayCore {
    /// The groups this device is in or was in, in the order made.
    pub async fn groups(&self) -> Result<Vec<RelayGroup>, RelayError> {
        let me = self.me().await?;
        let groups = self.record.get().groups.clone();
        Ok(groups.iter().map(|g| self.group_info(g, &me)).collect())
    }

    pub async fn group(&self, id: String) -> Result<RelayGroup, RelayError> {
        let me = self.me().await?;
        let g = self
            .record
            .get()
            .group(&id)
            .cloned()
            .ok_or_else(|| no_group(&id))?;
        Ok(self.group_info(&g, &me))
    }

    /// Makes a group named `name` with this device as admin and the
    /// trusted devices `members`, and tells each of them.
    pub async fn create_group(
        &self,
        name: String,
        members: Vec<String>,
    ) -> Result<RelayGroup, RelayError> {
        let name = crate::model::clean_name(&name)?;
        let me = self.me().await?;
        let mut group = SavedGroup {
            id: hex(&new_id()),
            name,
            admin: me.clone(),
            members: vec![GroupMember {
                endpoint_id: me.clone(),
                name: self.identity().name,
            }],
            version: 1,
            left: false,
        };
        self.add_trusted(&mut group, &me, &members)?;
        if group.members.len() < 2 {
            return Err(RelayError::Declined {
                reason: "A group needs at least one other member".into(),
            });
        }
        let (saved, _) = self.change_group(group, Vec::new(), &me).await?;
        Ok(self.group_info(&saved, &me))
    }

    /// Adds the trusted devices `members` (admin only).
    pub async fn add_group_members(
        &self,
        id: String,
        members: Vec<String>,
    ) -> Result<RelayGroup, RelayError> {
        let (me, mut group) = self.as_admin(&id).await?;
        self.add_trusted(&mut group, &me, &members)?;
        group.version += 1;
        let (saved, _) = self.change_group(group, Vec::new(), &me).await?;
        Ok(self.group_info(&saved, &me))
    }

    /// Removes `member` (admin only): it is told, and sent nothing after.
    pub async fn remove_group_member(
        &self,
        id: String,
        member: String,
    ) -> Result<RelayGroup, RelayError> {
        let (me, mut group) = self.as_admin(&id).await?;
        if member == me {
            return Err(RelayError::Declined {
                reason: "The admin leaves with Leave Group".into(),
            });
        }
        let Some(removed) = group.member(&member).cloned() else {
            return Err(RelayError::Declined {
                reason: "That device isn't in the group".into(),
            });
        };
        group.members.retain(|m| m.endpoint_id != member);
        group.version += 1;
        let (saved, _) = self.change_group(group, vec![removed], &me).await?;
        Ok(self.group_info(&saved, &me))
    }

    /// Renames the group (admin only).
    pub async fn rename_group(&self, id: String, name: String) -> Result<RelayGroup, RelayError> {
        let (me, mut group) = self.as_admin(&id).await?;
        group.name = crate::model::clean_name(&name)?;
        group.version += 1;
        let (saved, _) = self.change_group(group, Vec::new(), &me).await?;
        Ok(self.group_info(&saved, &me))
    }

    /// Leaves the group: every other member is told, and this device
    /// sends and takes nothing more in it. Its conversation stays.
    pub async fn leave_group(&self, id: String) -> Result<(), RelayError> {
        let me = self.me().await?;
        let others: Vec<String> = {
            let record = self.record.get();
            let g = record.group(&id).ok_or_else(|| no_group(&id))?;
            if g.left {
                return Ok(());
            }
            self.reachable_members(&record, g, &me)
        };
        let group = id.clone();
        self.rt
            .command("leave-group", move |state| {
                state.change(|r| {
                    if let Some(g) = r.group_mut(&group) {
                        g.left = true;
                    }
                    for to in &others {
                        r.queue_message(
                            &new_id(),
                            to,
                            Payload::GroupLeave {
                                group: group.clone(),
                            },
                            now_ms(),
                        );
                    }
                    Ok(others)
                })
            })
            .await?
            .iter()
            .for_each(|to| self.wake(to));
        Ok(())
    }

    /// How many members have our message `message_id`.
    pub fn group_delivery(
        &self,
        id: String,
        message_id: String,
    ) -> Result<GroupDelivery, RelayError> {
        let record = self.record.get();
        let g = record.group(&id).ok_or_else(|| no_group(&id))?;
        let conversation = group_conversation(&id);
        let delivered = self
            .peer_logs
            .store()
            .read(|c| crate::store::group_delivered(c, &conversation, &message_id))?;
        Ok(GroupDelivery {
            delivered: delivered.len() as u32,
            of: g.members.len().saturating_sub(1) as u32,
        })
    }
}

impl RelayCore {
    /// The group's messages, oldest first.
    pub fn group_messages(&self, id: &str) -> Result<Vec<Message>, RelayError> {
        self.peer_logs.get(&group_conversation(id))?.since(None)
    }

    /// Sends `text` to the group and waits for the first attempt at each
    /// member: the message comes back `Sent` once one has it, or `Queued`.
    pub async fn send_group_message(&self, id: &str, text: String) -> Result<Message, RelayError> {
        let body = crate::model::clean_note(&text)?;
        let message = Message {
            id: hex(&new_id()),
            author: self.identity().name,
            author_id: ME.into(),
            body,
            ..Message::default()
        };
        let (queued, firsts) = self.queue_group_message(id, message).await?;
        for first in firsts {
            let _ = first.await;
        }
        Ok(self
            .peer_logs
            .get(&group_conversation(id))?
            .get(&queued.id)?
            .unwrap_or(queued))
    }

    /// Puts one of our messages in the queue of each member it hasn't
    /// reached, and returns it `Queued`. Sending a failed message again
    /// tries the members that don't have it; one already on its way comes
    /// back as it is.
    pub(crate) async fn queue_group_message(
        &self,
        id: &str,
        message: Message,
    ) -> Result<(Message, Firsts), RelayError> {
        let me = self.me().await?;
        let recipients = {
            let record = self.record.get();
            let g = record.group(id).ok_or_else(|| no_group(id))?;
            if g.left {
                return Err(RelayError::Declined {
                    reason: "You're no longer in this group".into(),
                });
            }
            self.reachable_members(&record, g, &me)
        };
        if recipients.is_empty() {
            return Err(RelayError::Declined {
                reason: "No one in this group is a trusted device here yet".into(),
            });
        }
        let group = id.to_string();
        let conversation = group_conversation(id);
        let waiters = Arc::clone(&self.outbox.waiters);
        let (message, firsts, to_wake) = self
            .rt
            .command("queue-group-message", move |state| {
                state.change_with(|r, c| {
                    let message = match c.get(&conversation, &message.id)? {
                        Some(found) if found.status == MessageStatus::Failed => {
                            c.set_status(&conversation, &found.id, MessageStatus::Queued)?;
                            c.get(&conversation, &found.id)?.unwrap_or(found)
                        }
                        Some(found) => return Ok((found, Vec::new(), Vec::new())),
                        None => c.add(
                            &conversation,
                            Message {
                                status: MessageStatus::Queued,
                                ..message
                            },
                        )?,
                    };
                    let have =
                        crate::store::group_delivered(c.connection(), &conversation, &message.id)?;
                    let mut firsts = Vec::new();
                    let mut woken = Vec::new();
                    for to in recipients.iter().filter(|to| !have.contains(to)) {
                        let delivery = new_id();
                        r.queue_message(
                            &delivery,
                            to,
                            Payload::GroupMessage {
                                group: group.clone(),
                                id: message.id.clone(),
                                body: message.body.clone(),
                                reply_to: message.reply_to.clone(),
                            },
                            now_ms(),
                        );
                        let (tx, rx) = oneshot::channel();
                        lock(&waiters).entry(hex(&delivery)).or_default().push(tx);
                        firsts.push(rx);
                        woken.push(to.clone());
                    }
                    if woken.is_empty() {
                        // Everyone has it already.
                        c.set_status(&conversation, &message.id, MessageStatus::Sent)?;
                    }
                    Ok((message, firsts, woken))
                })
            })
            .await?;
        for to in &to_wake {
            self.wake(to);
        }
        Ok((message, firsts))
    }

    /// The members other than `me` that items go to: trusted devices here.
    fn reachable_members(&self, record: &Record, g: &SavedGroup, me: &str) -> Vec<String> {
        g.others(me)
            .filter(|m| {
                record
                    .devices
                    .get(&m.endpoint_id)
                    .is_some_and(|d| d.trust == SavedTrust::Trusted)
            })
            .map(|m| m.endpoint_id.clone())
            .collect()
    }

    /// The group `id` and this device's endpoint id, when this device is
    /// its admin and still in it.
    async fn as_admin(&self, id: &str) -> Result<(String, SavedGroup), RelayError> {
        let me = self.me().await?;
        let g = self
            .record
            .get()
            .group(id)
            .cloned()
            .ok_or_else(|| no_group(id))?;
        if g.admin != me || g.left {
            return Err(RelayError::Declined {
                reason: "Only the group's admin changes it".into(),
            });
        }
        Ok((me, g))
    }

    /// Adds each trusted device in `members` that isn't in `group` yet.
    fn add_trusted(
        &self,
        group: &mut SavedGroup,
        me: &str,
        members: &[String],
    ) -> Result<(), RelayError> {
        let record = self.record.get();
        for id in members {
            if id == me || group.member(id).is_some() {
                continue;
            }
            let device = record
                .devices
                .get(id)
                .filter(|d| d.trust == SavedTrust::Trusted)
                .ok_or_else(|| RelayError::UnknownPeer { id: id.clone() })?;
            if group.members.len() >= MAX_MEMBERS {
                return Err(RelayError::Declined {
                    reason: format!("A group has at most {MAX_MEMBERS} members"),
                });
            }
            group.members.push(GroupMember {
                endpoint_id: id.clone(),
                name: device.shown_name().to_string(),
            });
        }
        Ok(())
    }

    /// Saves `group` as changed by this device (its admin) and queues it
    /// for every member, and for `removed`, who learn they're out.
    async fn change_group(
        &self,
        group: SavedGroup,
        removed: Vec<GroupMember>,
        me: &str,
    ) -> Result<(SavedGroup, Firsts), RelayError> {
        let mut to: Vec<String> = {
            let record = self.record.get();
            self.reachable_members(&record, &group, me)
        };
        to.extend(removed.into_iter().map(|m| m.endpoint_id));
        let waiters = Arc::clone(&self.outbox.waiters);
        let saved = group.clone();
        let (saved, firsts, to) = self
            .rt
            .command("change-group", move |state| {
                state.change(|r| {
                    match r.group_mut(&saved.id) {
                        Some(g) => *g = saved.clone(),
                        None => r.groups.push(saved.clone()),
                    }
                    let mut firsts = Vec::new();
                    for member in &to {
                        let delivery = new_id();
                        r.queue_message(
                            &delivery,
                            member,
                            Payload::GroupUpdate {
                                group: saved.shared(),
                            },
                            now_ms(),
                        );
                        let (tx, rx) = oneshot::channel();
                        lock(&waiters).entry(hex(&delivery)).or_default().push(tx);
                        firsts.push(rx);
                    }
                    Ok((saved, firsts, to))
                })
            })
            .await?;
        for member in &to {
            self.wake(member);
        }
        Ok((saved, firsts))
    }

    /// The `ChatRepo` of group `id`'s conversation.
    pub(crate) fn group_repo(self: &Arc<Self>, id: &str) -> Result<GroupRepo, RelayError> {
        Ok(GroupRepo {
            relay: Arc::downgrade(self),
            id: id.to_string(),
            log: self.peer_logs.get(&group_conversation(id))?,
        })
    }
}

fn no_group(id: &str) -> RelayError {
    RelayError::Declined {
        reason: format!("No group {id} on this device"),
    }
}

/// Group item `payload` as it goes on `relay/device/1`.
pub(crate) fn outgoing(payload: &Payload) -> Option<crate::device::DeviceMsg> {
    use crate::device::DeviceMsg;
    let group_bytes =
        |g: &str| -> [u8; 16] { unhex(g).and_then(|b| b.try_into().ok()).unwrap_or([0; 16]) };
    Some(match payload {
        Payload::GroupMessage {
            group,
            id,
            body,
            reply_to,
        } => DeviceMsg::GroupMessage {
            group: group_bytes(group),
            id: id.clone(),
            body: body.clone(),
            reply_to: reply_to.clone(),
        },
        Payload::GroupUpdate { group } => DeviceMsg::GroupUpdate {
            group: group_bytes(&group.id),
            version: group.version,
            name: group.name.clone(),
            admin: group.admin.clone(),
            members: group
                .members
                .iter()
                .map(|m| (m.endpoint_id.clone(), m.name.clone()))
                .collect(),
        },
        Payload::GroupLeave { group } => DeviceMsg::GroupLeave {
            group: group_bytes(group),
        },
        _ => return None,
    })
}

/// The group item a message on `relay/device/1` carries, if it is one.
pub(crate) fn incoming(msg: crate::device::DeviceMsg) -> Option<GroupItem> {
    use crate::device::DeviceMsg;
    Some(match msg {
        DeviceMsg::GroupMessage {
            group,
            id,
            body,
            reply_to,
        } => GroupItem::Message {
            group: hex(&group),
            message: PeerMessage { id, body, reply_to },
        },
        DeviceMsg::GroupUpdate {
            group,
            version,
            name,
            admin,
            members,
        } => GroupItem::Update(SavedGroup {
            id: hex(&group),
            name,
            admin,
            members: members
                .into_iter()
                .take(MAX_MEMBERS + 1)
                .map(|(endpoint_id, name)| GroupMember { endpoint_id, name })
                .collect(),
            version,
            left: false,
        }),
        DeviceMsg::GroupLeave { group } => GroupItem::Leave { group: hex(&group) },
        _ => return None,
    })
}

/// The `ChatRepo` of one group's conversation.
pub struct GroupRepo {
    relay: Weak<RelayCore>,
    id: String,
    log: PeerLog,
}

impl GroupRepo {
    fn relay(&self) -> Result<Arc<RelayCore>, ChatError> {
        self.relay.upgrade().ok_or(ChatError::ShutDown)
    }

    fn check(&self, conversation_id: &str) -> Result<(), ChatError> {
        if conversation_group(conversation_id) == Some(self.id.as_str()) {
            Ok(())
        } else {
            Err(ChatError::NotFound {
                conversation_id: conversation_id.into(),
            })
        }
    }
}

impl ChatRepo for GroupRepo {
    fn list_conversations(&self) -> Result<Vec<RemoteConversation>, ChatError> {
        let relay = self.relay()?;
        let title = relay
            .record
            .get()
            .group(&self.id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Group".into());
        let last = self.log.last().map_err(chat_error)?;
        Ok(vec![RemoteConversation {
            id: group_conversation(&self.id),
            title,
            last_message_at: last.as_ref().map(|m| m.sent_at),
            last_message: last,
            read_up_to: None,
            others_read_up_to: None,
        }])
    }

    fn load_messages(
        &self,
        conversation_id: &str,
        since: Option<i64>,
    ) -> Result<Vec<Message>, ChatError> {
        self.check(conversation_id)?;
        self.log.since(since).map_err(chat_error)
    }

    fn post_message(&self, message: &OutgoingMessage) -> Result<Message, ChatError> {
        self.check(&message.conversation_id)?;
        if !message.attachments.is_empty() {
            return Err(ChatError::UnsupportedOnHost {
                detail: "photos and files in groups come in a later version".into(),
            });
        }
        let relay = self.relay()?;
        let queued = forge_runtime::block_on(relay.queue_group_message(
            &self.id,
            Message {
                id: message.id.clone(),
                author: message.author.clone(),
                author_id: message.author_id.clone(),
                body: message.body.clone(),
                reply_to: message.reply_to.clone(),
                ..Message::default()
            },
        ))
        .map_err(chat_error)?;
        Ok(queued.0)
    }

    fn open_stream(
        &self,
        conversation_id: &str,
        cancel: &CancelHandle,
    ) -> Result<Box<dyn RemoteStream>, ChatError> {
        self.check(conversation_id)?;
        self.log.stream(cancel)
    }

    fn download_attachment(
        &self,
        _id: &str,
        _destination: &std::path::Path,
    ) -> Result<(), ChatError> {
        Err(ChatError::InvalidInput {
            reason: "the file is no longer on this device".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(admin: &str, members: &[&str], version: u64) -> SavedGroup {
        SavedGroup {
            id: "0123456789abcdef0123456789abcdef".into(),
            name: "Family".into(),
            admin: admin.into(),
            members: members
                .iter()
                .map(|m| GroupMember {
                    endpoint_id: m.to_string(),
                    name: m.to_string(),
                })
                .collect(),
            version,
            left: false,
        }
    }

    fn take(r: &mut Record, me: &str, from: &str, item: GroupItem) -> Result<(), RelayError> {
        let store = crate::store::Store::in_memory();
        store.write(|c| {
            let mut conversations = Conversations::new(c);
            r.take_group_item(&mut conversations, me, from, item)
        })
    }

    #[test]
    fn only_the_admin_changes_a_group_and_only_forward() {
        let mut r = Record::default();
        // Not from its admin: refused.
        assert!(take(
            &mut r,
            "me",
            "b",
            GroupItem::Update(group("a", &["a", "b", "me"], 1))
        )
        .is_err());
        take(
            &mut r,
            "me",
            "a",
            GroupItem::Update(group("a", &["a", "b", "me"], 1)),
        )
        .unwrap();
        assert_eq!(r.groups.len(), 1);
        // An older version is a retry: nothing changes.
        take(
            &mut r,
            "me",
            "a",
            GroupItem::Update(group("a", &["a", "me"], 1)),
        )
        .unwrap();
        assert_eq!(r.groups[0].members.len(), 3);
        // Another device claiming to be admin of the same group: refused.
        assert!(take(
            &mut r,
            "me",
            "b",
            GroupItem::Update(group("b", &["b", "me"], 5))
        )
        .is_err());
        // Removed: kept, marked left.
        take(
            &mut r,
            "me",
            "a",
            GroupItem::Update(group("a", &["a", "b"], 2)),
        )
        .unwrap();
        assert!(r.groups[0].left);
    }

    #[test]
    fn a_group_one_isnt_in_is_not_kept() {
        let mut r = Record::default();
        take(
            &mut r,
            "me",
            "a",
            GroupItem::Update(group("a", &["a", "b"], 1)),
        )
        .unwrap();
        assert!(r.groups.is_empty());
    }

    #[test]
    fn a_damaged_update_is_refused() {
        let mut r = Record::default();
        let mut g = group("a", &["a", "me"], 1);
        g.id = "nothex".into();
        assert!(take(&mut r, "me", "a", GroupItem::Update(g)).is_err());
        let many: Vec<String> = (0..=MAX_MEMBERS).map(|i| format!("m{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let mut g = group("m0", &refs, 1);
        g.members.push(GroupMember {
            endpoint_id: "me".into(),
            name: "me".into(),
        });
        assert!(take(&mut r, "me", "m0", GroupItem::Update(g)).is_err());
        // The admin must be a member.
        assert!(take(
            &mut r,
            "me",
            "a",
            GroupItem::Update(group("a", &["b", "me"], 1))
        )
        .is_err());
    }

    #[test]
    fn only_members_post_and_a_member_who_leaves_stops() {
        let mut r = Record::default();
        let store = crate::store::Store::in_memory();
        let g = group("a", &["a", "b", "me"], 1);
        let id = g.id.clone();
        let msg = |text: &str| GroupItem::Message {
            group: id.clone(),
            message: PeerMessage {
                id: text.into(),
                body: text.into(),
                reply_to: None,
            },
        };
        store
            .write(|c| {
                let mut cs = Conversations::new(c);
                r.take_group_item(&mut cs, "me", "a", GroupItem::Update(g.clone()))?;
                r.take_group_item(&mut cs, "me", "b", msg("hello"))?;
                assert!(r
                    .take_group_item(&mut cs, "me", "stranger", msg("x"))
                    .is_err());
                r.take_group_item(&mut cs, "me", "b", GroupItem::Leave { group: id.clone() })?;
                assert!(r.take_group_item(&mut cs, "me", "b", msg("again")).is_err());
                Ok::<_, RelayError>(())
            })
            .unwrap();
        let messages = store
            .read(|c| crate::store::since(c, &group_conversation(&id), None))
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "hello");
        assert_eq!(messages[0].author_id, "b");
    }

    #[test]
    fn group_ids_and_conversations() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(conversation_group(&group_conversation(id)), Some(id));
        assert_eq!(conversation_group("group:../x"), None);
        assert_eq!(conversation_group("device:abc"), None);
    }
}
