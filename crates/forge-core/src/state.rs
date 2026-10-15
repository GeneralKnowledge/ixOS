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

    pub fn validate(&self) -> Result<(), crate::CounterError> {
        if self.count < COUNTER_MIN {
            return Err(crate::CounterError::WouldUnderflow {
                minimum: COUNTER_MIN,
            });
        }
        if self.count > COUNTER_MAX {
            return Err(crate::CounterError::WouldOverflow {
                maximum: COUNTER_MAX,
            });
        }
        Ok(())
    }
}
