//! Trusted devices: who may use `relay/device/1`, and what the pairing
//! policy does after a word-code transfer (brief PR 3,
//! examples/relay/docs/relay-m1-m2-brief.md, "One transfer path").
//!
//! Pure rules, no I/O, so the revocation rule is tested here exactly:
//!
//! - A device is trusted by its iroh endpoint id, never by its name.
//! - **Revoking** ends trust for good: no trusted connection is accepted,
//!   and no policy, code or earlier pairing makes the device trusted again
//!   by itself. Only the person can, by trusting it afresh. A one-off
//!   word-code transfer still works, and never offers or restores trust.
//! - **Forgetting** removes the device from the list but remembers that it
//!   was trusted, so the Pair Automatically policy can trust it again after
//!   a code transfer. A revoked device forgotten stays revoked.

use serde::{Deserialize, Serialize};

use crate::resolver::{ConnectionMode, RelayServers, RouteKind};

/// What happens after a word-code transfer with a device (Settings).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum PairingPolicy {
    /// A code moves one transfer; nothing is remembered.
    CodesOnly,
    /// After a code transfer, offer to trust the other device.
    #[default]
    AskToPair,
    /// A device trusted before (then forgotten) is trusted again by itself;
    /// a new device is offered, as with Ask to Pair.
    PairAutomatically,
}

/// Where a device stands with this one.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedTrust {
    Trusted,
    Revoked,
    /// Trusted once, then forgotten by the person: not listed, not
    /// accepted, but known to Pair Automatically.
    Forgotten,
}

/// A device this one has trusted, keyed by its endpoint id.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedDevice {
    /// The iroh endpoint id, as iroh prints it.
    pub endpoint_id: String,
    /// The name it had when it was trusted (what it called itself).
    pub name: String,
    pub trust: SavedTrust,
    pub since_ms: i64,
    /// Whether it may send files and notes here (on by default).
    #[serde(default = "yes")]
    pub receives_from: bool,
    /// Whether it may ask this device's AI (off by default): trust never
    /// grants it on its own.
    #[serde(default)]
    pub may_ask: bool,
    /// Where it was last reached or seen, for when Bonjour hasn't found it.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// When it was last reached, in ms since the epoch: something was
    /// delivered to it, or it answered a status request (brief PR 5).
    #[serde(default)]
    pub last_seen_ms: Option<i64>,
    /// What it last said about itself on `relay/device/1` (`Status`): its
    /// platform, such as "ios" or "linux", and what it offers, such as
    /// "notes" or "files". Its own claim, shown read-only; it grants
    /// nothing here.
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub offers: Vec<String>,
    /// Where it said it can be reached, the last time the two met: its
    /// Tailscale address too, which Bonjour never shows (M2 PR 6).
    #[serde(default)]
    pub told: Vec<String>,
    /// The route of the last connection with it, either way.
    #[serde(default)]
    pub route: Option<RouteKind>,
    /// Whether this device introduces devices here (My Devices, step 1):
    /// the devices it trusts are offered to the person, and the ones it
    /// revokes are revoked here. Off until the person turns it on.
    #[serde(default)]
    pub introduces: bool,
    /// Whether it may see the devices trusted here, so this device can
    /// introduce them to it. Off until the person turns it on: trust alone
    /// never shares the list.
    #[serde(default)]
    pub sees_devices: bool,
    /// The introducer that recommended it, when the person trusted it from
    /// that recommendation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_by: Option<String>,
    /// The introducer that revoked it here, when it wasn't the person: the
    /// person can see it, and trust it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_by: Option<String>,
    /// The person this device belongs to, when it isn't one of the
    /// person's own devices (N4 step 1). A person's device sends messages
    /// and files, but never introduces devices or sees the device list,
    /// and Chats shows this name rather than the device's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
}

impl SavedDevice {
    /// The name Chats shows: the person's, or else the device's.
    pub fn shown_name(&self) -> &str {
        self.person.as_deref().unwrap_or(&self.name)
    }
}

/// What an introducer says about its devices (My Devices, step 1).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceList {
    /// The devices it trusts: endpoint id and the name it knows them by.
    pub trusted: Vec<(String, String)>,
    /// The devices it has revoked.
    pub revoked: Vec<String>,
}

/// What an introducer's list did here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Introduced {
    /// Devices to offer the person: endpoint id and name.
    pub offers: Vec<(String, String)>,
    /// Devices revoked here because the introducer revoked them.
    pub revoked: Vec<String>,
}

fn yes() -> bool {
    true
}

/// What to do after a code transfer with a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterCode {
    /// Nothing: codes only, already trusted, or revoked.
    Nothing,
    /// Ask the person whether to trust it.
    Offer,
    /// Trust it again without asking (Pair Automatically, known before).
    TrustAgain,
}

/// The pairing policy's answer after a code transfer with a device whose
/// standing here is `known` (`None`: never trusted).
pub fn after_code(policy: PairingPolicy, known: Option<SavedTrust>) -> AfterCode {
    match (policy, known) {
        (_, Some(SavedTrust::Revoked)) => AfterCode::Nothing,
        (_, Some(SavedTrust::Trusted)) => AfterCode::Nothing,
        (PairingPolicy::CodesOnly, _) => AfterCode::Nothing,
        (PairingPolicy::PairAutomatically, Some(SavedTrust::Forgotten)) => AfterCode::TrustAgain,
        (PairingPolicy::AskToPair | PairingPolicy::PairAutomatically, _) => AfterCode::Offer,
    }
}

/// Whether a `relay/device/1` connection from a device with this standing
/// is accepted.
pub fn may_connect(known: Option<SavedTrust>) -> bool {
    known == Some(SavedTrust::Trusted)
}

/// The devices this one knows, with the rules applied.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Devices {
    #[serde(default)]
    pub policy: PairingPolicy,
    /// Which routes trusted devices are reached by (M2 PR 6).
    #[serde(default)]
    pub mode: ConnectionMode,
    /// Relay's own servers (M2 PR 7). On by default since N2.
    #[serde(default)]
    pub servers: RelayServers,
    /// The servers were set by the person, or the N2 default was applied
    /// once: Relay never changes them by itself again.
    #[serde(default)]
    pub servers_settled: bool,
    #[serde(default)]
    pub list: Vec<SavedDevice>,
}

impl Devices {
    pub fn get(&self, endpoint_id: &str) -> Option<&SavedDevice> {
        self.list.iter().find(|d| d.endpoint_id == endpoint_id)
    }

    fn get_mut(&mut self, endpoint_id: &str) -> Option<&mut SavedDevice> {
        self.list.iter_mut().find(|d| d.endpoint_id == endpoint_id)
    }

    pub fn standing(&self, endpoint_id: &str) -> Option<SavedTrust> {
        self.get(endpoint_id).map(|d| d.trust)
    }

    /// The person trusts the device (afresh, if it was revoked): the only
    /// way a revoked device becomes trusted again.
    pub fn trust(&mut self, endpoint_id: &str, name: &str, now_ms: i64) {
        match self.get_mut(endpoint_id) {
            Some(d) => {
                d.trust = SavedTrust::Trusted;
                d.since_ms = now_ms;
                d.receives_from = true;
                d.may_ask = false;
                d.introduces = false;
                d.sees_devices = false;
                d.introduced_by = None;
                d.revoked_by = None;
                if !name.is_empty() {
                    d.name = name.into();
                }
            }
            None => self.list.push(SavedDevice {
                endpoint_id: endpoint_id.into(),
                name: name.into(),
                trust: SavedTrust::Trusted,
                since_ms: now_ms,
                receives_from: true,
                may_ask: false,
                addresses: Vec::new(),
                last_seen_ms: None,
                platform: None,
                offers: Vec::new(),
                told: Vec::new(),
                route: None,
                introduces: false,
                sees_devices: false,
                introduced_by: None,
                revoked_by: None,
                person: None,
            }),
        }
    }

    /// Trusts a device the person accepted from `introducer`'s
    /// recommendation, and remembers who recommended it.
    pub fn trust_introduced(
        &mut self,
        endpoint_id: &str,
        name: &str,
        introducer: &str,
        now_ms: i64,
    ) {
        self.trust(endpoint_id, name, now_ms);
        if let Some(d) = self.get_mut(endpoint_id) {
            d.introduced_by = Some(introducer.to_string());
        }
    }

    /// Marks a trusted device as a person's (`Some(name)`) or as one of
    /// the person's own devices (`None`) (N4 step 1). Marking it a
    /// person's turns off introducing and seeing the device list, which a
    /// person's device never has. Returns false for a device that isn't
    /// trusted, or an empty name.
    pub fn set_person(&mut self, endpoint_id: &str, person: Option<&str>) -> bool {
        let person = person.map(str::trim);
        if person.is_some_and(str::is_empty) {
            return false;
        }
        match self.get_mut(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted => {
                d.person = person.map(str::to_string);
                if d.person.is_some() {
                    d.introduces = false;
                    d.sees_devices = false;
                }
                true
            }
            _ => false,
        }
    }

    /// Whether the device introduces devices here. Only for a trusted
    /// device, and never for a person's; returns false otherwise.
    pub fn set_introduces(&mut self, endpoint_id: &str, on: bool) -> bool {
        match self.get_mut(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted && !(on && d.person.is_some()) => {
                d.introduces = on;
                true
            }
            _ => false,
        }
    }

    /// Whether the device may see the devices trusted here. Only for a
    /// trusted device, and never for a person's; returns false otherwise.
    pub fn set_sees_devices(&mut self, endpoint_id: &str, on: bool) -> bool {
        match self.get_mut(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted && !(on && d.person.is_some()) => {
                d.sees_devices = on;
                true
            }
            _ => false,
        }
    }

    /// The list this device gives `asker`: the devices trusted and revoked
    /// here, without the asker itself. People's devices are left out of
    /// the trusted ones, so they are never offered as the person's own;
    /// a revoked one is still listed, so it is revoked everywhere.
    pub fn list_for(&self, asker: &str) -> DeviceList {
        let mut list = DeviceList::default();
        for d in self.list.iter().filter(|d| d.endpoint_id != asker) {
            match d.trust {
                SavedTrust::Trusted if d.person.is_some() => {}
                SavedTrust::Trusted => list.trusted.push((d.endpoint_id.clone(), d.name.clone())),
                SavedTrust::Revoked => list.revoked.push(d.endpoint_id.clone()),
                SavedTrust::Forgotten => {}
            }
        }
        list
    }

    /// Applies `list` from `introducer` (My Devices, step 1). Its word
    /// counts only while it is trusted here and introduces devices.
    /// Devices it trusts that aren't trusted here are offered, never
    /// trusted by themselves: a recommendation, which the person decides.
    /// A device revoked here is never offered again. Devices it revoked
    /// are revoked here, marked with who did it, so the person can see and
    /// undo it; it can't revoke itself or this device (`me`).
    pub fn introduced(&mut self, introducer: &str, me: &str, list: &DeviceList) -> Introduced {
        let mut done = Introduced::default();
        if !self
            .get(introducer)
            .is_some_and(|d| d.trust == SavedTrust::Trusted && d.introduces)
        {
            return done;
        }
        for id in &list.revoked {
            if id == introducer || id == me {
                continue;
            }
            if let Some(d) = self.get_mut(id) {
                if d.trust == SavedTrust::Trusted {
                    d.trust = SavedTrust::Revoked;
                    d.addresses.clear();
                    d.revoked_by = Some(introducer.to_string());
                    done.revoked.push(id.clone());
                }
            }
        }
        for (id, name) in &list.trusted {
            if id == me || id == introducer {
                continue;
            }
            match self.standing(id) {
                Some(SavedTrust::Trusted) | Some(SavedTrust::Revoked) => {}
                None | Some(SavedTrust::Forgotten) => {
                    done.offers.push((id.clone(), name.clone()));
                }
            }
        }
        done
    }

    /// Applies the pairing policy after a code transfer. Returns what it
    /// decided; `TrustAgain` has already been applied.
    pub fn after_code(&mut self, endpoint_id: &str, name: &str, now_ms: i64) -> AfterCode {
        let decision = after_code(self.policy, self.standing(endpoint_id));
        if decision == AfterCode::TrustAgain {
            self.trust(endpoint_id, name, now_ms);
        }
        decision
    }

    /// Ends trust. Returns false for a device that was never trusted here.
    pub fn revoke(&mut self, endpoint_id: &str) -> bool {
        match self.get_mut(endpoint_id) {
            Some(d) => {
                d.trust = SavedTrust::Revoked;
                d.addresses.clear();
                d.revoked_by = None;
                true
            }
            None => false,
        }
    }

    /// Takes the device off the list. A trusted one is remembered for Pair
    /// Automatically; a revoked one stays revoked (and hidden).
    pub fn forget(&mut self, endpoint_id: &str) -> bool {
        match self.get_mut(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted => {
                d.trust = SavedTrust::Forgotten;
                d.addresses.clear();
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// Whether the device may send files and notes here. Only for a
    /// trusted device; returns false otherwise.
    pub fn set_receives_from(&mut self, endpoint_id: &str, on: bool) -> bool {
        match self.get_mut(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted => {
                d.receives_from = on;
                true
            }
            _ => false,
        }
    }

    /// Whether the device may ask this device's AI (off until the person
    /// turns it on). Only for a trusted device; returns false otherwise.
    pub fn set_may_ask(&mut self, endpoint_id: &str, on: bool) -> bool {
        match self.get_mut(endpoint_id) {
            Some(d) if d.trust == SavedTrust::Trusted => {
                d.may_ask = on;
                true
            }
            _ => false,
        }
    }

    /// Remembers where and when a trusted device was reached.
    pub fn seen_at(&mut self, endpoint_id: &str, addresses: &[String], now_ms: i64) {
        if let Some(d) = self.get_mut(endpoint_id) {
            if d.trust == SavedTrust::Trusted {
                if !addresses.is_empty() {
                    d.addresses = addresses.to_vec();
                }
                d.last_seen_ms = Some(d.last_seen_ms.map_or(now_ms, |t| t.max(now_ms)));
            }
        }
    }

    /// Remembers what a trusted device said about itself (its `Status`).
    /// A revoked or unknown device's claims aren't kept.
    pub fn reported(&mut self, endpoint_id: &str, platform: &str, offers: &[String], now_ms: i64) {
        if let Some(d) = self.get_mut(endpoint_id) {
            if d.trust == SavedTrust::Trusted {
                d.platform = (!platform.is_empty()).then(|| platform.to_string());
                d.offers = offers.to_vec();
                d.last_seen_ms = Some(d.last_seen_ms.map_or(now_ms, |t| t.max(now_ms)));
            }
        }
    }

    /// A trusted device said where it can be reached, and a connection with
    /// it used `route`.
    pub fn met(&mut self, endpoint_id: &str, told: &[String], route: Option<RouteKind>) {
        if let Some(d) = self.get_mut(endpoint_id) {
            if d.trust == SavedTrust::Trusted {
                let told = crate::resolver::clean(told);
                if !told.is_empty() {
                    d.told = told;
                }
                if route.is_some() {
                    d.route = route;
                }
            }
        }
    }

    /// Trusted and revoked devices, as the person sees the list.
    pub fn listed(&self) -> impl Iterator<Item = &SavedDevice> {
        self.list
            .iter()
            .filter(|d| d.trust != SavedTrust::Forgotten)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_introducer_offers_and_revokes_but_never_trusts_or_unseats_itself() {
        let mut devices = Devices::default();
        devices.trust("laptop", "Laptop", 1);
        devices.trust("tablet", "Tablet", 1);
        let list = DeviceList {
            trusted: vec![
                ("new".into(), "New".into()),
                ("me".into(), "Me".into()),
                ("tablet".into(), "Tablet".into()),
            ],
            revoked: vec!["laptop".into(), "me".into(), "tablet".into()],
        };
        // Not an introducer yet: its word counts for nothing.
        assert_eq!(
            devices.introduced("laptop", "me", &list),
            Introduced::default()
        );
        assert!(devices.set_introduces("laptop", true));
        let done = devices.introduced("laptop", "me", &list);
        assert_eq!(done.offers, [("new".to_string(), "New".to_string())]);
        assert_eq!(done.revoked, ["tablet"]);
        assert_eq!(devices.standing("laptop"), Some(SavedTrust::Trusted));
        assert_eq!(devices.standing("new"), None, "offered, not trusted");
        let tablet = devices.get("tablet").unwrap();
        assert_eq!(tablet.revoked_by.as_deref(), Some("laptop"));
        // A revoked device isn't offered again.
        let list = DeviceList {
            trusted: vec![("tablet".into(), "Tablet".into())],
            revoked: vec![],
        };
        assert!(devices.introduced("laptop", "me", &list).offers.is_empty());
        // The list given to a device leaves that device out.
        let given = devices.list_for("tablet");
        assert!(given.trusted.iter().all(|(id, _)| id != "tablet"));
        assert!(!given.revoked.contains(&"tablet".to_string()));
    }

    #[test]
    fn a_persons_device_never_introduces_or_sees_the_list() {
        let mut devices = Devices::default();
        devices.trust("sam", "iPhone", 1);
        devices.trust("laptop", "Laptop", 1);
        assert!(devices.set_sees_devices("sam", true));
        assert!(devices.set_introduces("sam", true));
        assert!(!devices.set_person("sam", Some("  ")), "a name is needed");
        assert!(devices.set_person("sam", Some(" Sam ")));
        let sam = devices.get("sam").unwrap();
        assert_eq!(sam.shown_name(), "Sam");
        assert!(!sam.introduces && !sam.sees_devices, "turned off");
        assert!(!devices.set_introduces("sam", true));
        assert!(!devices.set_sees_devices("sam", true));
        assert!(devices.set_sees_devices("sam", false));
        // It is never offered to the person's own devices as theirs.
        let given = devices.list_for("laptop");
        assert!(given.trusted.is_empty());
        // Trusting it afresh keeps whose it is; revoking it is shared.
        devices.trust("sam", "iPhone", 2);
        assert_eq!(devices.get("sam").unwrap().shown_name(), "Sam");
        assert!(devices.revoke("sam"));
        assert_eq!(devices.list_for("laptop").revoked, ["sam"]);
        assert!(!devices.set_person("sam", None), "only while trusted");
        // One of the person's own devices shows its own name.
        assert!(devices.set_person("laptop", None));
        assert_eq!(devices.get("laptop").unwrap().shown_name(), "Laptop");
    }
    use super::*;
    use AfterCode::*;
    use PairingPolicy::*;
    use SavedTrust::*;

    #[test]
    fn the_policy_after_a_code_transfer() {
        for policy in [CodesOnly, AskToPair, PairAutomatically] {
            // Never offered or restored for a revoked device, whatever the policy.
            assert_eq!(after_code(policy, Some(Revoked)), Nothing, "{policy:?}");
            assert_eq!(after_code(policy, Some(Trusted)), Nothing, "{policy:?}");
        }
        assert_eq!(after_code(CodesOnly, None), Nothing);
        assert_eq!(after_code(CodesOnly, Some(Forgotten)), Nothing);
        assert_eq!(after_code(AskToPair, None), Offer);
        assert_eq!(after_code(AskToPair, Some(Forgotten)), Offer);
        assert_eq!(after_code(PairAutomatically, None), Offer);
        assert_eq!(after_code(PairAutomatically, Some(Forgotten)), TrustAgain);
    }

    #[test]
    fn only_a_trusted_device_may_connect() {
        assert!(may_connect(Some(Trusted)));
        assert!(!may_connect(Some(Revoked)));
        assert!(!may_connect(Some(Forgotten)));
        assert!(!may_connect(None));
    }

    #[test]
    fn a_revoked_device_is_never_trusted_again_by_itself() {
        let mut d = Devices {
            policy: PairAutomatically,
            ..Devices::default()
        };
        d.trust("laptop", "Laptop", 1);
        assert!(d.revoke("laptop"));
        // A code transfer under the automatic policy: still revoked.
        assert_eq!(d.after_code("laptop", "Laptop", 2), Nothing);
        assert_eq!(d.standing("laptop"), Some(Revoked));
        // Forgetting doesn't launder it into "known before".
        assert!(d.forget("laptop"));
        assert_eq!(d.after_code("laptop", "Laptop", 3), Nothing);
        assert_eq!(d.standing("laptop"), Some(Revoked));
        assert!(!d.set_receives_from("laptop", true));
        // Only the person can trust it afresh.
        d.trust("laptop", "Laptop", 4);
        assert_eq!(d.standing("laptop"), Some(Trusted));
    }

    #[test]
    fn a_forgotten_device_is_trusted_again_only_by_the_automatic_policy() {
        let mut d = Devices::default();
        d.trust("phone", "Phone", 1);
        assert!(d.forget("phone"));
        assert_eq!(d.listed().count(), 0);
        assert!(!may_connect(d.standing("phone")));
        assert_eq!(d.after_code("phone", "Phone", 2), Offer);
        assert_eq!(d.standing("phone"), Some(Forgotten));
        d.policy = PairAutomatically;
        assert_eq!(d.after_code("phone", "Phone 2", 3), TrustAgain);
        assert_eq!(d.standing("phone"), Some(Trusted));
        assert_eq!(d.get("phone").unwrap().name, "Phone 2");
    }

    #[test]
    fn revoking_or_forgetting_drops_where_it_was() {
        let mut d = Devices::default();
        d.trust("a", "A", 1);
        d.seen_at("a", &["192.168.1.2:4000".into()], 5);
        assert_eq!(d.get("a").unwrap().addresses.len(), 1);
        d.revoke("a");
        assert!(d.get("a").unwrap().addresses.is_empty());
        d.seen_at("a", &["192.168.1.2:4000".into()], 6);
        assert!(d.get("a").unwrap().addresses.is_empty());
        assert!(!d.revoke("nobody"));
        assert!(!d.forget("nobody"));
    }

    #[test]
    fn last_seen_only_moves_forward_and_status_is_kept_for_trusted_devices() {
        let mut d = Devices::default();
        d.trust("a", "A", 1);
        d.seen_at("a", &[], 50);
        d.seen_at("a", &[], 40);
        assert_eq!(d.get("a").unwrap().last_seen_ms, Some(50));
        d.reported("a", "ios", &["notes".into(), "files".into()], 60);
        let a = d.get("a").unwrap();
        assert_eq!(a.platform.as_deref(), Some("ios"));
        assert_eq!(a.offers, ["notes", "files"]);
        assert_eq!(a.last_seen_ms, Some(60));
        d.revoke("a");
        d.reported("a", "linux", &[], 70);
        d.seen_at("a", &[], 80);
        let a = d.get("a").unwrap();
        assert_eq!(
            a.platform.as_deref(),
            Some("ios"),
            "a revoked device's claims aren't kept"
        );
        assert_eq!(a.last_seen_ms, Some(60));
        d.reported("nobody", "linux", &[], 1);
    }

    #[test]
    fn an_old_record_reads_as_ask_to_pair_with_no_devices() {
        let d: Devices = serde_json::from_str("{}").unwrap();
        assert_eq!(d.policy, AskToPair);
        assert!(d.list.is_empty());
        let one: SavedDevice = serde_json::from_str(
            r#"{"endpoint_id":"x","name":"X","trust":"Trusted","since_ms":1}"#,
        )
        .unwrap();
        assert!(one.receives_from);
        assert!(!one.may_ask, "an old record grants no asks");
        assert_eq!(one.last_seen_ms, None);
        assert_eq!(one.platform, None);
        assert!(one.offers.is_empty());
    }
}
