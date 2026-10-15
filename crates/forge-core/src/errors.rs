use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CounterError {
    #[error("count cannot go below {minimum}")]
    WouldUnderflow { minimum: i64 },
    #[error("count cannot exceed {maximum}")]
    WouldOverflow { maximum: i64 },
}
