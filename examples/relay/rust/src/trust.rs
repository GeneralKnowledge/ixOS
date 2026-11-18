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
    /// Where it was last reached or seen, for when Bonjour hasn't found it.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// Where it said it can be reached, the last time the two met: its
    /// Tailscale address too, which Bonjour never shows (M2 PR 6).
    #[serde(default)]
    pub told: Vec<String>,
    /// The route of the last connection with it, either way.
    #[serde(default)]
    pub route: Option<RouteKind>,
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
    /// Relay's own servers (M2 PR 7); none until they are set.
    #[serde(default)]
    pub servers: RelayServers,
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
                addresses: Vec::new(),
                told: Vec::new(),
                route: None,
            }),
        }
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

    /// Remembers where a trusted device was reached or seen.
    pub fn seen_at(&mut self, endpoint_id: &str, addresses: &[String]) {
        if let Some(d) = self.get_mut(endpoint_id) {
            if d.trust == SavedTrust::Trusted && !addresses.is_empty() {
                d.addresses = addresses.to_vec();
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
        d.seen_at("a", &["192.168.1.2:4000".into()]);
        assert_eq!(d.get("a").unwrap().addresses.len(), 1);
        d.revoke("a");
        assert!(d.get("a").unwrap().addresses.is_empty());
        d.seen_at("a", &["192.168.1.2:4000".into()]);
        assert!(d.get("a").unwrap().addresses.is_empty());
        assert!(!d.revoke("nobody"));
        assert!(!d.forget("nobody"));
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
    }
}
