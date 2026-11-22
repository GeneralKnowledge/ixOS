//! The review-and-confirm rule every proposal surface shares (A2UI-1's,
//! now used by the app surfaces too): a change set is good only for the
//! revision it was made at. Any accepted payload, any edit on the surface
//! and any newer review drops it, so Confirm can't apply what the person
//! didn't see.

use super::A2uiLabError;

pub struct Reviews<P> {
    revision: u64,
    pending: Option<(u64, P)>,
}

impl<P> Default for Reviews<P> {
    fn default() -> Self {
        Self {
            revision: 0,
            pending: None,
        }
    }
}

impl<P> Reviews<P> {
    /// A payload was accepted: a new revision, and no reviewed change set.
    pub fn accepted(&mut self) -> u64 {
        self.revision += 1;
        self.pending = None;
        self.revision
    }

    /// A review produced `plan`: it gets its own revision.
    pub fn reviewed(&mut self, plan: P) -> u64 {
        self.revision += 1;
        self.pending = Some((self.revision, plan));
        self.revision
    }

    /// The person changed something on the surface. Returns whether a
    /// reviewed change set was dropped.
    pub fn edited(&mut self) -> bool {
        self.pending.take().is_some()
    }

    /// The plan reviewed at `revision`, if nothing changed since.
    pub fn confirm(&mut self, revision: u64) -> Result<P, A2uiLabError> {
        let stale = || A2uiLabError::Stale {
            reason: "This changed after you reviewed it. Review it again.".into(),
        };
        match &self.pending {
            Some((at, _)) if *at == revision && revision == self.revision => {}
            _ => return Err(stale()),
        }
        // A stale revision leaves the current plan in place; only a match
        // takes it, so it applies once.
        self.pending.take().map(|(_, plan)| plan).ok_or_else(stale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_set_is_good_for_its_own_revision_only() {
        let mut r = Reviews::default();
        r.accepted();
        let first = r.reviewed("a");
        let second = r.reviewed("b");
        assert!(r.confirm(first).is_err());
        let third = r.reviewed("c");
        assert!(second < third);
        assert_eq!(r.confirm(third).unwrap(), "c");
        assert!(r.confirm(third).is_err(), "applies once");
        let fourth = r.reviewed("d");
        assert!(r.edited());
        assert!(r.confirm(fourth).is_err());
        let fifth = r.reviewed("e");
        r.accepted();
        assert!(r.confirm(fifth).is_err());
    }

    #[test]
    fn a_stale_confirm_keeps_the_current_plan() {
        let mut r = Reviews::default();
        r.accepted();
        let old = r.reviewed("old");
        let current = r.reviewed("current");
        assert!(r.confirm(old).is_err());
        assert_eq!(r.confirm(current).unwrap(), "current");
    }
}
