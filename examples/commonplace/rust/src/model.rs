//! What the SwiftUI app sees: records, the timeline filter and errors.

use forge_app::{ErrorKind, OpenError, Stopped};
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
    /// The folder it is in, if any.
    pub folder_id: Option<i64>,
    /// Whether its new articles count for the new-articles notification.
    pub notify: bool,
    /// Whether its articles should be fetched in full from their pages.
    /// Kept (and carried through OPML) until full-article fetching exists.
    pub full_text: bool,
}

/// A folder of feeds. Folders can hold folders.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    /// How deep it is: 0 for a folder at the top.
    pub depth: u32,
    /// Its feeds, including those in folders inside it.
    pub feed_count: u32,
    pub unread_count: u32,
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

/// What the reader shows for one article: its details, and its body as a
/// ForgeReader section (headings, paragraphs, quotes, lists, code and
/// pictures, with bold, italic and links), converted from the feed's HTML.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ArticleText {
    pub id: i64,
    pub feed_title: String,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_at_ms: i64,
    pub body: crate::ReaderSection,
    pub read: bool,
    pub bookmarked: bool,
    /// The body is the full article from its web page, not the feed's text.
    pub full: bool,
}

/// The person's choices, kept in the library.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AppSettings {
    /// Timelines list the oldest article first.
    pub oldest_first: bool,
    /// The All Articles and feed timelines leave read articles out (All
    /// Unread and Bookmarks are unchanged).
    pub hide_read: bool,
    /// Timeline rows show the start of each article under its title.
    pub show_summaries: bool,
    /// Links in an article open inside the app, in a web view; otherwise
    /// in Safari.
    pub open_links_in_app: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            oldest_first: false,
            hide_read: false,
            show_summaries: true,
            open_links_in_app: true,
        }
    }
}

/// Which articles a timeline shows.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum Timeline {
    All,
    Unread,
    Bookmarked,
    Feed {
        feed_id: i64,
    },
    /// Every feed in a folder, and in the folders inside it.
    Folder {
        folder_id: i64,
    },
}

/// One feed an OPML file lists, as the import screen shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OpmlFeed {
    /// Its place in the file: what `import_opml` is given to pick it.
    pub index: u32,
    pub title: String,
    pub url: String,
    /// The folders it sits in, outermost first.
    pub folder: Vec<String>,
    pub status: OpmlFeedStatus,
    /// Settings it carries that Commonplace doesn't use (kept for export).
    pub not_used: Vec<String>,
}

/// What importing one listed feed would do.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum OpmlFeedStatus {
    /// Subscribed to on import.
    New,
    /// Already in the library: left exactly as it is.
    AlreadySubscribed { feed_id: i64 },
    /// Listed earlier in the same file.
    ListedTwice,
    /// Not a web address Commonplace can fetch.
    BadAddress,
}

/// What an OPML file holds, before anything is imported.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OpmlPreview {
    pub feeds: Vec<OpmlFeed>,
    /// Folder paths in the file, "Outer / Inner".
    pub folders: Vec<String>,
    /// Filter patterns it holds that passed the checks, and those refused.
    pub filters: u32,
    pub rejected_filters: u32,
    /// Its global settings Commonplace doesn't carry over.
    pub settings_not_used: Vec<String>,
}

/// What an import did.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OpmlImport {
    /// New subscriptions (their articles arrive with the next refresh).
    pub added: u32,
    /// Picked feeds already in the library, left exactly as they were.
    pub already_subscribed: u32,
    pub folders_created: u32,
    pub filters_added: u32,
    /// Picked feeds that were listed twice or had a bad address.
    pub skipped: u32,
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
    /// Full articles fetched from their pages, for feeds that want them.
    pub full_articles: u32,
}

/// Every error the API returns. `reason` fields are diagnostics, not UI copy.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("not found")]
    NotFound,
    /// The article's page has no text to read (a video, a sign-in page,
    /// something too large), or isn't a web page.
    #[error("no readable text: {reason}")]
    NoReadableText { reason: String },
    /// A file that isn't OPML (or is too large to be a subscription list).
    #[error("not OPML: {reason}")]
    NotOpml { reason: String },
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

// Network failures keep their category (forge-app's shared mapping).
forge_app::app_error_conversions!(AppError: NetError);

impl AppError {
    /// What a feed's last refresh hit, as stored on the feed for the screen
    /// to word (`Feed.last_error`): the kind, and an HTTP status.
    pub fn problem(&self) -> String {
        match self {
            Self::Http { status } => format!("http-{status}"),
            other => other.kind().to_string(),
        }
    }

    /// The variant's name, for logs (never the reason, which may hold an
    /// address the user typed).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. } => "invalid-input",
            Self::NotFound => "not-found",
            Self::NotOpml { .. } => "not-opml",
            Self::NoReadableText { .. } => "no-readable-text",
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
        AppError::InvalidInput { .. }
        | AppError::NotAFeed { .. }
        | AppError::NotOpml { .. }
        | AppError::NoReadableText { .. } => ErrorKind::Invalid,
        AppError::NotFound => ErrorKind::NotFound,
        AppError::AlreadySubscribed { .. } => ErrorKind::Invalid,
        AppError::Storage { .. } => ErrorKind::Internal,
        // The site answered with an error, or the library is open elsewhere.
        AppError::InUse | AppError::Http { .. } => ErrorKind::Unavailable,
        AppError::Offline { .. }
        | AppError::Certificate { .. }
        | AppError::Timeout
        | AppError::Network { .. } => ErrorKind::Network,
        AppError::Cancelled => ErrorKind::Cancelled,
        AppError::Interrupted => ErrorKind::Interrupted,
        AppError::ShutDown => ErrorKind::ShutDown,
    }
}
