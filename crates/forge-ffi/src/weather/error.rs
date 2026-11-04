use forge_net::NetError;

/// Every error the Weather API returns. Grouped like `ForgeAppError` (see
/// `docs/ffi-policy.md`); `reason` fields are diagnostics, not UI copy.
#[derive(uniffi::Error, Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum WeatherError {
    // Input. Nothing was sent.
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    // Network. A refresh only fails with these when no usable cached report
    // exists; otherwise it succeeds with `Freshness::Cached`.
    /// The service could not be reached (no connection, DNS, refused).
    #[error("offline: {reason}")]
    Offline { reason: String },
    #[error("the request timed out")]
    Timeout,
    /// The service answered with a non-2xx status. `message` is its own
    /// explanation, when it sent one.
    #[error("server returned HTTP {status}")]
    Server {
        status: u16,
        message: Option<String>,
    },
    /// The response was not the JSON this build expects.
    #[error("unexpected response: {reason}")]
    BadResponse { reason: String },
    /// TLS and other transport failures that are not "offline".
    #[error("network error: {reason}")]
    Network { reason: String },
    // Cancellation. Not a failure.
    #[error("operation was cancelled")]
    Cancelled,
    // Lifecycle.
    #[error("the weather app instance has been shut down")]
    ShutDown,
    // Internal. A bug, not an expected condition.
    #[error("internal failure: {reason}")]
    Internal { reason: String },
}

impl WeatherError {
    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            reason: reason.into(),
        }
    }

    pub(crate) fn bad_response(reason: impl std::fmt::Display) -> Self {
        Self::BadResponse {
            reason: reason.to_string(),
        }
    }

    /// The [`super::FetchProblem`] to report when this error is masked by a
    /// cached report, or `None` if a cache must not mask it.
    pub(crate) fn as_fetch_problem(&self) -> Option<super::FetchProblem> {
        use super::FetchProblem;
        match self {
            Self::Offline { .. } | Self::Network { .. } => Some(FetchProblem::Offline),
            Self::Timeout => Some(FetchProblem::TimedOut),
            Self::Server { status, .. } if *status >= 500 => {
                Some(FetchProblem::ServerError { status: *status })
            }
            Self::BadResponse { .. } => Some(FetchProblem::BadResponse),
            // A 4xx means the request itself is wrong; stale data would hide it.
            _ => None,
        }
    }
}

impl From<NetError> for WeatherError {
    fn from(value: NetError) -> Self {
        match value {
            NetError::Unavailable => Self::Offline {
                reason: "networking is not available in this build".into(),
            },
            NetError::InvalidRequest { reason } => Self::InvalidInput { reason },
            NetError::Offline { reason } => Self::Offline { reason },
            NetError::Transport { reason } => Self::Network { reason },
            NetError::Timeout => Self::Timeout,
            NetError::Cancelled => Self::Cancelled,
        }
    }
}
