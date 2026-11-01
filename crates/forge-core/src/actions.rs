use crate::errors::CounterError;
use crate::state::{CounterSnapshot, COUNTER_MAX, COUNTER_MIN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterAction {
    Increment,
    Decrement,
    Reset,
    /// Adds `amount` in one step (used by long-running bulk operations).
    Add(u32),
}

#[derive(Debug, Clone)]
pub struct CounterEngine {
    snapshot: CounterSnapshot,
}

impl Default for CounterEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterEngine {
    pub fn new() -> Self {
        Self {
            snapshot: CounterSnapshot::initial(),
        }
    }

    pub fn from_snapshot(snapshot: CounterSnapshot) -> Result<Self, CounterError> {
        snapshot.validate()?;
        Ok(Self { snapshot })
    }

    pub fn snapshot(&self) -> &CounterSnapshot {
        &self.snapshot
    }

    pub fn apply(&mut self, action: CounterAction) -> Result<CounterSnapshot, CounterError> {
        let before = self.snapshot.clone();
        let next = match action {
            CounterAction::Increment => {
                if before.count >= COUNTER_MAX {
                    return Err(CounterError::WouldOverflow {
                        maximum: COUNTER_MAX,
                    });
                }
                CounterSnapshot {
                    count: before.count + 1,
                }
            }
            CounterAction::Decrement => {
                if before.count <= COUNTER_MIN {
                    return Err(CounterError::WouldUnderflow {
                        minimum: COUNTER_MIN,
                    });
                }
                CounterSnapshot {
                    count: before.count - 1,
                }
            }
            CounterAction::Reset => CounterSnapshot::initial(),
            CounterAction::Add(amount) => {
                let next = before.count + i64::from(amount);
                if next > COUNTER_MAX {
                    return Err(CounterError::WouldOverflow {
                        maximum: COUNTER_MAX,
                    });
                }
                CounterSnapshot { count: next }
            }
        };
        next.validate()?;
        self.snapshot = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_from_zero() {
        let mut engine = CounterEngine::new();
        let snap = engine.apply(CounterAction::Increment).unwrap();
        assert_eq!(snap.count, 1);
    }

    #[test]
    fn decrement_at_zero_rejected() {
        let mut engine = CounterEngine::new();
        let err = engine.apply(CounterAction::Decrement).unwrap_err();
        assert_eq!(
            err,
            CounterError::WouldUnderflow {
                minimum: COUNTER_MIN
            }
        );
        assert_eq!(engine.snapshot().count, 0);
    }

    #[test]
    fn sequence_and_reset() {
        let mut engine = CounterEngine::new();
        engine.apply(CounterAction::Increment).unwrap();
        engine.apply(CounterAction::Increment).unwrap();
        engine.apply(CounterAction::Decrement).unwrap();
        assert_eq!(engine.snapshot().count, 1);
        engine.apply(CounterAction::Reset).unwrap();
        assert_eq!(engine.snapshot().count, 0);
    }

    #[test]
    fn add_applies_the_whole_amount() {
        let mut engine = CounterEngine::new();
        assert_eq!(engine.apply(CounterAction::Add(250)).unwrap().count, 250);
        assert_eq!(engine.apply(CounterAction::Add(0)).unwrap().count, 250);
    }

    #[test]
    fn add_past_max_is_rejected_whole() {
        let mut engine = CounterEngine::from_snapshot(CounterSnapshot {
            count: COUNTER_MAX - 5,
        })
        .unwrap();
        let err = engine.apply(CounterAction::Add(6)).unwrap_err();
        assert!(matches!(err, CounterError::WouldOverflow { .. }));
        assert_eq!(engine.snapshot().count, COUNTER_MAX - 5);
        assert_eq!(
            engine.apply(CounterAction::Add(5)).unwrap().count,
            COUNTER_MAX
        );
    }

    #[test]
    fn max_enforced() {
        let mut engine =
            CounterEngine::from_snapshot(CounterSnapshot { count: COUNTER_MAX }).unwrap();
        let err = engine.apply(CounterAction::Increment).unwrap_err();
        assert!(matches!(err, CounterError::WouldOverflow { .. }));
        assert_eq!(engine.snapshot().count, COUNTER_MAX);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn rejected_actions_preserve_state(ops in proptest::collection::vec(prop_oneof![
            Just(CounterAction::Increment),
            Just(CounterAction::Decrement),
            Just(CounterAction::Reset),
            (0u32..300_000).prop_map(CounterAction::Add),
        ], 1..40)) {
            let mut engine = CounterEngine::new();
            for op in ops {
                let before = engine.snapshot().clone();
                match engine.apply(op) {
                    Ok(after) => {
                        prop_assert!(after.count >= COUNTER_MIN && after.count <= COUNTER_MAX);
                        prop_assert_eq!(engine.snapshot(), &after);
                    }
                    Err(_) => {
                        prop_assert_eq!(engine.snapshot(), &before);
                    }
                }
            }
        }

        #[test]
        fn count_always_in_range(ops in proptest::collection::vec(prop_oneof![
            Just(CounterAction::Increment),
            Just(CounterAction::Decrement),
            Just(CounterAction::Reset),
            (0u32..300_000).prop_map(CounterAction::Add),
        ], 0..50)) {
            let mut engine = CounterEngine::new();
            for op in ops {
                let _ = engine.apply(op);
                prop_assert!(engine.snapshot().count >= COUNTER_MIN);
                prop_assert!(engine.snapshot().count <= COUNTER_MAX);
            }
        }
    }
}
