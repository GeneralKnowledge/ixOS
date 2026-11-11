//! What the SwiftUI app sees: records, the timeline filter and errors.

use forge_app::{ErrorKind, OpenError, Stopped};
use forge_net::NetError;
use forge_storage::StorageError;

/// A subscription.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    pub id: i64,
    /// Where the feed is fetched from.
    pub url: String,
    /// The person's name for it, else the feed's own title.
    pub title: String,
    /// The website the feed belongs to, when it says.
    pub site_url: Option<String>,
    pub unread_count: u32,
    pub article_count: u32,
    /// Unix time in milliseconds of the last successful refresh.
    pub refreshed_at_ms: Option<i64>,
    /// Why the last refresh failed, if it did. A diagnostic, not UI copy.
    pub last_error: Option<String>,
}

/// One article in a timeline: what a row shows.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ArticleSummary {
    pub id: i64,
    pub feed_id: i64,
    pub feed_title: String,
    pub title: String,
    /// The start of the article as plain text, at most about 200 characters.
    pub summary: String,
    pub url: Option<String>,
    pub author: Option<String>,
    /// When the feed says it was published (else when it was first fetched),
    /// Unix time in milliseconds.
    pub published_at_ms: i64,
    pub read: bool,
    pub bookmarked: bool,
}

/// What the reader shows for one article. This is the boundary the native
/// reader (docs/native-capability-roadmap.md, Tier 1) will replace: for now
/// the body is plain paragraphs, with no styling, images or links inside.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ArticleText {
    pub id: i64,
    pub feed_title: String,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_at_ms: i64,
    pub paragraphs: Vec<String>,
    pub read: bool,
    pub bookmarked: bool,
}

/// Which articles a timeline shows.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum Timeline {
    All,
    Unread,
    Bookmarked,
    Feed { feed_id: i64 },
}

/// A feed that could not be refreshed.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct FeedFailure {
    pub feed_id: i64,
    pub title: String,
    pub error: AppError,
}

/// What a refresh did.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct RefreshReport {
    pub feeds: u32,
    /// Feeds the server said had not changed (HTTP 304).
    pub unchanged: u32,
    pub new_articles: u32,
    pub failures: Vec<FeedFailure>,
}

/// Every error the API returns. `reason` fields are diagnostics, not UI copy.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("not found")]
    NotFound,
    /// Already subscribed to this address.
    #[error("already subscribed")]
    AlreadySubscribed { feed_id: i64 },
    /// The address answered, but not with an RSS, Atom or JSON feed.
    #[error("not a feed: {reason}")]
    NotAFeed { reason: String },
    /// The server answered with an HTTP error.
    #[error("HTTP {status}")]
    Http { status: u16 },
    #[error("storage error: {reason}")]
    Storage { reason: String },
    #[error("the library is open in another copy of the app")]
    InUse,
    #[error("offline: {reason}")]
    Offline { reason: String },
    #[error("certificate rejected: {reason}")]
    Certificate { reason: String },
    #[error("request timed out")]
    Timeout,
    #[error("network error: {reason}")]
    Network { reason: String },
    #[error("cancelled")]
    Cancelled,
    #[error("interrupted: the app went to the background")]
    Interrupted,
    #[error("the app has been shut down")]
    ShutDown,
}

impl From<Stopped> for AppError {
    fn from(stopped: Stopped) -> Self {
        match stopped.kind {
            ErrorKind::Interrupted => Self::Interrupted,
            ErrorKind::ShutDown => Self::ShutDown,
            _ => Self::Cancelled,
        }
    }
}

impl From<StorageError> for AppError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<forge_storage::rusqlite::Error> for AppError {
    fn from(err: forge_storage::rusqlite::Error) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<OpenError> for AppError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

impl From<NetError> for AppError {
    fn from(err: NetError) -> Self {
        match err {
            NetError::InvalidRequest { reason } => Self::InvalidInput { reason },
            NetError::Offline { reason } => Self::Offline { reason },
            NetError::Certificate { reason } => Self::Certificate { reason },
            NetError::Timeout => Self::Timeout,
            NetError::Cancelled => Self::Cancelled,
            other => Self::Network {
                reason: other.to_string(),
            },
        }
    }
}

impl AppError {
    /// The variant's name, for logs (never the reason, which may hold an
    /// address the user typed).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. } => "invalid-input",
            Self::NotFound => "not-found",
            Self::AlreadySubscribed { .. } => "already-subscribed",
            Self::NotAFeed { .. } => "not-a-feed",
            Self::Http { .. } => "http",
            Self::Storage { .. } => "storage",
            Self::InUse => "in-use",
            Self::Offline { .. } => "offline",
            Self::Certificate { .. } => "certificate",
            Self::Timeout => "timeout",
            Self::Network { .. } => "network",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::ShutDown => "shut-down",
        }
    }
}

/// The framework kind of an error, so ForgeKit can present any of them.
#[uniffi::export]
pub fn app_error_kind(error: AppError) -> ErrorKind {
    match error {
        AppError::InvalidInput { .. } | AppError::NotAFeed { .. } => ErrorKind::Invalid,
        AppError::NotFound => ErrorKind::NotFound,
        AppError::AlreadySubscribed { .. } => ErrorKind::Invalid,
        AppError::Storage { .. } => ErrorKind::Internal,
        AppError::InUse
        | AppError::Http { .. }
        | AppError::Offline { .. }
        | AppError::Certificate { .. }
        | AppError::Timeout
        | AppError::Network { .. } => ErrorKind::Unavailable,
        AppError::Cancelled => ErrorKind::Cancelled,
        AppError::Interrupted => ErrorKind::Interrupted,
        AppError::ShutDown => ErrorKind::ShutDown,
    }
}
