use forge_app::{ErrorKind, OpenError, Stopped};
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
    /// The service answered with a non-2xx status. `reason` is its own
    /// explanation, when it sent one.
    #[error("server returned HTTP {status}")]
    Server { status: u16, reason: Option<String> },
    /// The response was not the JSON this build expects.
    #[error("unexpected response: {reason}")]
    BadResponse { reason: String },
    /// The service's TLS certificate was rejected (expired, untrusted, or
    /// for another host). A cached report never masks this.
    #[error("certificate rejected: {reason}")]
    Certificate { reason: String },
    /// Transport failures that are not "offline" or a certificate problem.
    #[error("network error: {reason}")]
    Network { reason: String },
    // Location ("My Location").
    /// The person said no to location, or turned it off for the app.
    #[error("location is not allowed")]
    LocationDenied,
    /// No position could be found (indoors, no signal, timed out).
    #[error("location unavailable: {reason}")]
    LocationUnavailable { reason: String },
    // Cancellation. Not a failure. Also what a refresh or search gets when
    // the app leaves the screen: the page refreshes again on return.
    #[error("operation was cancelled")]
    Cancelled,
    // Lifecycle.
    #[error("the weather app instance has been shut down")]
    ShutDown,
    /// Another Weather core already has this data folder open.
    #[error("the weather data is open elsewhere")]
    InUse,
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

/// The framework kind of an error, so ForgeKit presents every app's errors
/// alike (`ForgeKindedError` in Swift).
#[uniffi::export]
pub fn weather_error_kind(error: WeatherError) -> ErrorKind {
    match error {
        WeatherError::InvalidInput { .. } => ErrorKind::Invalid,
        WeatherError::Offline { .. }
        | WeatherError::Timeout
        | WeatherError::Certificate { .. }
        | WeatherError::Network { .. } => ErrorKind::Network,
        WeatherError::Server { status, .. } if status < 500 => ErrorKind::Refused,
        WeatherError::Server { .. } | WeatherError::BadResponse { .. } | WeatherError::InUse => {
            ErrorKind::Unavailable
        }
        WeatherError::LocationDenied => ErrorKind::NeedsUser,
        WeatherError::LocationUnavailable { .. } => ErrorKind::Unavailable,
        WeatherError::Cancelled => ErrorKind::Cancelled,
        WeatherError::ShutDown => ErrorKind::ShutDown,
        WeatherError::Internal { .. } => ErrorKind::Internal,
    }
}

/// The runtime stopped the work: shutdown refuses, anything else (the app
/// left the screen, the host cancelled) is a cancellation.
impl From<Stopped> for WeatherError {
    fn from(stopped: Stopped) -> Self {
        match stopped.kind {
            ErrorKind::ShutDown => Self::ShutDown,
            _ => Self::Cancelled,
        }
    }
}

impl From<OpenError> for WeatherError {
    fn from(error: OpenError) -> Self {
        match error {
            OpenError::InUse { .. } => Self::InUse,
            OpenError::Io { reason } => Self::Internal { reason },
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
            NetError::Certificate { reason } => Self::Certificate { reason },
            // The host skipped ForgeAndroid.init: a wiring bug, not the network.
            NetError::TlsNotInitialised { reason } => Self::Internal { reason },
            NetError::Timeout => Self::Timeout,
            NetError::Cancelled => Self::Cancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_errors_are_typed_and_never_masked_by_the_cache() {
        let err = WeatherError::from(NetError::Certificate {
            reason: "expired".into(),
        });
        assert!(matches!(err, WeatherError::Certificate { .. }));
        assert_eq!(err.as_fetch_problem(), None);
        assert!(matches!(
            WeatherError::from(NetError::TlsNotInitialised {
                reason: "init".into()
            }),
            WeatherError::Internal { .. }
        ));
    }
}
