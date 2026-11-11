@preconcurrency import ForgeFFI
import Foundation

// Presentation-only additions to the Rust records: identity for SwiftUI
// lists and dates as `Date`. Rules stay in Rust.

extension Feed: Identifiable {}
extension ArticleSummary: Identifiable {}

extension ArticleSummary {
    public var published: Date { Date(timeIntervalSince1970: TimeInterval(publishedAtMs) / 1000) }
}

extension ArticleText {
    public var published: Date { Date(timeIntervalSince1970: TimeInterval(publishedAtMs) / 1000) }
}

extension Feed {
    public var refreshedAt: Date? {
        refreshedAtMs.map { Date(timeIntervalSince1970: TimeInterval($0) / 1000) }
    }
}

/// Where the feed list can lead.
public enum Route: Hashable {
    case timeline(Timeline)
    case article(Int64)
}

extension Timeline {
    /// The timeline's screen title.
    @MainActor
    public func title(in model: LibraryModel) -> String {
        switch self {
        case .all: return "All Articles"
        case .unread: return "All Unread"
        case .bookmarked: return "Bookmarks"
        case .feed(let feedId): return model.feed(id: feedId)?.title ?? "Feed"
        }
    }

    /// An identifier fragment for the timeline's controls.
    public var identifier: String {
        switch self {
        case .all: return "all"
        case .unread: return "unread"
        case .bookmarked: return "bookmarks"
        case .feed: return "feed"
        }
    }

    /// The feed it shows, if it is one feed.
    public var feedID: Int64? {
        if case .feed(let id) = self { return id }
        return nil
    }
}
