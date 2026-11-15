//! The sender's code session as a pure state machine, with no I/O.
//!
//! It mirrors `verification/relay/CodeSession.lean` transition for
//! transition: `attempt`, `finish`, `expire`, `reissue`, and the `answer`
//! a receiver gets. The network code in `main.rs` only calls these, so the
//! tests below exercise exactly the rules the model proves:
//!
//! 1. A code allows at most one guess (one admitted handshake).
//! 2. At most one receiver is allowed on a code.
//! 3. "busy" and "gone" answers change nothing.
//! 4. A spent or expired code is refused until a new code replaces it.
//!
//! Plus what the Lean model leaves to the code: every admitted handshake
//! ends with exactly one `finish`, and events about an old code are
//! recognised as stale by their generation.

/// What a receiver's first message gets back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Admitted: the handshake runs on this code.
    Go,
    /// Another handshake is running; the code is not spent.
    Busy,
    /// The code was used or has expired.
    Gone,
}

/// How an admitted handshake ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The receiver proved the code; it may fetch the file.
    Proved,
    /// The proof was wrong or malformed, or never came.
    WrongCode,
    /// The connection failed after the proof (or before ours was sent).
    Dropped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// Which code is live; bumped by `reissue`.
    pub gen: u64,
    pub spent: bool,
    pub busy: bool,
    pub expired: bool,
    /// Handshakes admitted on this code.
    pub admitted: u32,
    /// Receivers allowed to fetch because of this code.
    pub allowed: u32,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Session {
            gen: 0,
            spent: false,
            busy: false,
            expired: false,
            admitted: 0,
            allowed: 0,
        }
    }

    pub fn answer(&self) -> Answer {
        if self.busy {
            Answer::Busy
        } else if self.spent || self.expired {
            Answer::Gone
        } else {
            Answer::Go
        }
    }

    /// A receiver's first message arrived. On `Go` the handshake is admitted
    /// and the caller must later call [`Session::finish`] with the returned
    /// generation, whatever happens.
    pub fn attempt(&mut self) -> (Answer, u64) {
        let answer = self.answer();
        if answer == Answer::Go {
            self.busy = true;
            self.spent = true;
            self.admitted += 1;
        }
        (answer, self.gen)
    }

    /// The admitted handshake on code `gen` ended. Ignored (and `false`) if
    /// that code has since been replaced or this handshake already finished;
    /// only a `true` with `Proved` lets the receiver fetch.
    pub fn finish(&mut self, gen: u64, ending: Ending) -> bool {
        if gen != self.gen || !self.busy {
            return false;
        }
        self.busy = false;
        if ending == Ending::Proved {
            self.allowed += 1;
        }
        true
    }

    pub fn expire(&mut self) {
        self.expired = true;
    }

    /// Another sender took this code's number. Only an unused, live code is
    /// given up: the code is expired at once, under the same lock as the
    /// check, so no receiver can be admitted on it between the check and
    /// the reissue. Returns whether the caller should issue a new code.
    /// A collision reported for a code that has since been used (or
    /// replaced) is ignored, so it cannot cut off a handshake in progress.
    /// In the Lean model this is `expire`, taken only when the code is
    /// unspent.
    pub fn collided(&mut self, gen: u64) -> bool {
        if gen != self.gen || self.spent || self.expired {
            return false;
        }
        self.expired = true;
        true
    }

    /// A new code replaces this one.
    pub fn reissue(&mut self) {
        *self = Session {
            gen: self.gen + 1,
            ..Session::new()
        };
    }

    /// Whether an event about code `gen` still concerns the live code.
    pub fn is_current(&self, gen: u64) -> bool {
        gen == self.gen
    }

    /// The model's invariant, checked by the tests after every step.
    #[cfg(test)]
    pub fn invariant_holds(&self) -> bool {
        self.admitted <= 1
            && self.allowed <= self.admitted
            && ((self.admitted == 0) == !self.spent)
            && (!self.busy || (self.spent && self.allowed == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_code_admits_one_attempt() {
        let mut s = Session::new();
        assert_eq!(s.attempt(), (Answer::Go, 0));
        assert!(s.busy && s.spent);
        assert_eq!(s.admitted, 1);
    }

    #[test]
    fn busy_changes_nothing() {
        let mut s = Session::new();
        s.attempt();
        let before = s.clone();
        assert_eq!(s.attempt().0, Answer::Busy);
        assert_eq!(s, before);
    }

    #[test]
    fn a_spent_code_is_gone_and_changes_nothing() {
        let mut s = Session::new();
        let (_, gen) = s.attempt();
        s.finish(gen, Ending::WrongCode);
        let before = s.clone();
        assert_eq!(s.attempt().0, Answer::Gone);
        assert_eq!(s, before);
    }

    #[test]
    fn an_expired_code_is_gone() {
        let mut s = Session::new();
        s.expire();
        let before = s.clone();
        assert_eq!(s.attempt().0, Answer::Gone);
        assert_eq!(s, before);
    }

    #[test]
    fn a_proved_handshake_allows_one_receiver() {
        let mut s = Session::new();
        let (_, gen) = s.attempt();
        s.finish(gen, Ending::Proved);
        assert_eq!(s.allowed, 1);
        assert!(!s.busy);
    }

    #[test]
    fn wrong_and_dropped_allow_nobody() {
        for ending in [Ending::WrongCode, Ending::Dropped] {
            let mut s = Session::new();
            let (_, gen) = s.attempt();
            s.finish(gen, ending);
            assert_eq!(s.allowed, 0);
            assert!(!s.busy, "{ending:?} must clear busy");
            assert_eq!(
                s.answer(),
                Answer::Gone,
                "{ending:?} must not revive the code"
            );
        }
    }

    #[test]
    fn finishing_twice_counts_once() {
        let mut s = Session::new();
        let (_, gen) = s.attempt();
        assert!(s.finish(gen, Ending::Proved));
        assert!(!s.finish(gen, Ending::Proved));
        assert_eq!(s.allowed, 1);
    }

    #[test]
    fn reissue_gives_a_fresh_code_with_a_new_generation() {
        let mut s = Session::new();
        let (_, gen) = s.attempt();
        s.finish(gen, Ending::WrongCode);
        s.expire();
        s.reissue();
        assert_eq!(s.gen, 1);
        assert_eq!(s.answer(), Answer::Go);
        assert_eq!((s.admitted, s.allowed), (0, 0));
    }

    #[test]
    fn a_handshake_from_an_old_code_cannot_touch_the_new_one() {
        let mut s = Session::new();
        let (_, old) = s.attempt();
        // A collision or expiry replaced the code while it ran.
        s.reissue();
        assert!(
            !s.finish(old, Ending::Proved),
            "a stale proof must not admit"
        );
        assert_eq!(s.allowed, 0);
        assert_eq!(s.answer(), Answer::Go);
    }

    #[test]
    fn stale_events_are_recognised() {
        let mut s = Session::new();
        let first = s.gen;
        s.reissue();
        assert!(!s.is_current(first));
        assert!(s.is_current(s.gen));
    }

    #[test]
    fn a_collision_on_an_unused_code_expires_it_at_once() {
        let mut s = Session::new();
        assert!(s.collided(s.gen));
        assert_eq!(
            s.attempt().0,
            Answer::Gone,
            "no receiver between the check and the reissue"
        );
        assert!(!s.collided(s.gen), "reported once");
    }

    #[test]
    fn a_collision_queued_before_a_receiver_was_admitted_is_ignored() {
        let mut s = Session::new();
        // The collision is seen while the code is unused...
        let queued = s.gen;
        // ...but a receiver is admitted before the sender handles it.
        let (answer, gen) = s.attempt();
        assert_eq!(answer, Answer::Go);
        let before = s.clone();
        assert!(!s.collided(queued), "a code in use must not be replaced");
        assert_eq!(s, before);
        assert!(
            s.finish(gen, Ending::Proved),
            "the handshake still completes"
        );
        assert_eq!(s.allowed, 1);
    }

    #[test]
    fn a_collision_after_a_finished_attempt_is_ignored() {
        let mut s = Session::new();
        let queued = s.gen;
        let (_, gen) = s.attempt();
        s.finish(gen, Ending::WrongCode);
        assert!(!s.collided(queued));
    }

    #[test]
    fn a_collision_about_a_replaced_code_is_ignored() {
        let mut s = Session::new();
        let old = s.gen;
        s.reissue();
        assert!(!s.collided(old));
        assert_eq!(s.answer(), Answer::Go);
    }

    /// Every sequence of steps keeps the invariant, refusals change nothing,
    /// and a spent or expired code never answers `Go` before a reissue.
    /// A small deterministic generator, so no extra dependency.
    #[test]
    fn random_sequences_keep_the_model_properties() {
        let mut seed: u64 = 0x5eed_f00d;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for _ in 0..2_000 {
            let mut s = Session::new();
            let mut open: Vec<u64> = Vec::new();
            for _ in 0..60 {
                let was_unusable = s.spent || s.expired;
                let gen_before = s.gen;
                match next(6) {
                    0 => {
                        let before = s.clone();
                        let (answer, gen) = s.attempt();
                        if answer == Answer::Go {
                            open.push(gen);
                        } else {
                            assert_eq!(s, before, "a refusal changed the session");
                        }
                        if was_unusable {
                            assert_ne!(answer, Answer::Go, "a spent or expired code admitted");
                        }
                    }
                    1 => {
                        if let Some(gen) = open.pop() {
                            let ending = match next(3) {
                                0 => Ending::Proved,
                                1 => Ending::WrongCode,
                                _ => Ending::Dropped,
                            };
                            s.finish(gen, ending);
                        }
                    }
                    2 => s.expire(),
                    5 => {
                        // A collision report, possibly queued long ago.
                        let gen = s.gen.saturating_sub(next(2));
                        let was_in_use = s.busy || s.spent;
                        let before = s.clone();
                        if s.collided(gen) {
                            assert!(!was_in_use, "a collision replaced a code in use");
                            assert_eq!(s.answer(), Answer::Gone);
                        } else {
                            assert_eq!(s, before, "an ignored collision changed the session");
                        }
                    }
                    3 => s.reissue(),
                    _ => {
                        // A stale finish from an older code.
                        if s.gen > 0 {
                            s.finish(s.gen - 1, Ending::Proved);
                        }
                    }
                }
                assert!(s.invariant_holds(), "invariant broken: {s:?}");
                if s.gen == gen_before && was_unusable {
                    assert_ne!(s.answer(), Answer::Go, "a code came back without a reissue");
                }
            }
        }
    }
}
