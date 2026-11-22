//! Capabilities (N5, docs/relay-capabilities.md): a service a device
//! offers to its trusted devices, each on its own protocol and behind its
//! own grant.
//!
//! - **Trust admits; it grants nothing.** Every capability is off for
//!   every device until the person turns it on for that device, and
//!   trusting a device again starts it with none.
//! - **Checked where it is used.** The service checks the grant when a
//!   connection opens and again on every request, so taking it away (or
//!   revoking the device) ends access at once.
//! - **Seen only by who may use it.** A device learns that another offers
//!   a capability only when it has the grant (the `offers` of a status
//!   answer), so a device without it can't tell the service exists.
//!
//! Grants are saved by name (`SavedDevice::grants`), so a new capability
//! needs no new field. The rights from before N5 (may send here, may ask,
//! introduces, sees the devices) keep their own fields.

/// A capability one device may grant another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Capability {
    /// May open the sites this device shares (N6, `relay/web/1`).
    Sites,
}

impl Capability {
    pub const ALL: [Capability; 1] = [Capability::Sites];

    /// The name it is saved and offered by.
    pub fn name(self) -> &'static str {
        match self {
            Capability::Sites => "sites",
        }
    }

    /// The capability `name` names, if this version knows it. Names from a
    /// newer version are kept in the record but grant nothing here.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_names_grant_nothing() {
        for c in Capability::ALL {
            assert_eq!(Capability::from_name(c.name()), Some(c));
        }
        assert_eq!(Capability::Sites.name(), "sites");
        assert_eq!(Capability::from_name("terminal"), None);
        assert_eq!(Capability::from_name(""), None);
    }
}
