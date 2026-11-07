use serde::{Deserialize, Serialize};

pub const COUNTER_MIN: i64 = 0;
pub const COUNTER_MAX: i64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterSnapshot {
    pub count: i64,
}

impl CounterSnapshot {
    pub fn initial() -> Self {
        Self { count: COUNTER_MIN }
    }

    pub fn validate(&self) -> Result<(), super::CounterError> {
        if self.count < COUNTER_MIN {
            return Err(super::CounterError::WouldUnderflow {
                minimum: COUNTER_MIN,
            });
        }
        if self.count > COUNTER_MAX {
            return Err(super::CounterError::WouldOverflow {
                maximum: COUNTER_MAX,
            });
        }
        Ok(())
    }
}
