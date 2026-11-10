@preconcurrency import ForgeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Library")

/// The app's presentation state: the feeds, the refresh in progress, and
/// the actions the screens can take. It holds no rules. Every change goes
/// to the Rust `AppCore` (rust/src/lib.rs), and what the screens show comes
/// from Rust. It imports no SwiftUI, so it compiles and tests on Linux.
@Observable
@MainActor
public final class LibraryModel {
    /// Every subscription, by title, as Rust last reported it.
    public private(set) var feeds: [Feed] = []
    /// Moves whenever Rust's data changed, so views that read timelines
    /// (`timeline(_:)` asks Rust each time) draw again.
    public private(set) var version = 0
    public private(set) var isRefreshing = false
    /// What the last refresh did, for the status line under the feeds.
    public private(set) var lastRefresh: RefreshSummary?
    /// A failed action, presented by the feed list with `.forgeAlert`.
    public var alert: ForgeAlert<Never>?

    public let core: AppCore
    @ObservationIgnored private var refreshing: Cancellation?

    public init(core: AppCore) {
        self.core = core
        feeds = core.feeds()
    }

    /// Re-reads from Rust. Cheap: Rust hands over its published snapshot.
    public func reload() {
        feeds = core.feeds()
        version += 1
    }

    // MARK: Reading

    /// The articles a timeline shows, newest first.
    public func timeline(_ timeline: Timeline) -> [ArticleSummary] {
        _ = version
        return core.timeline(timeline: timeline)
    }

    public func counts(_ timeline: Timeline) -> TimelineCounts {
        _ = version
        return core.counts(timeline: timeline)
    }

    public func article(id: Int64) -> ArticleSummary? {
        _ = version
        return core.article(id: id)
    }

    public func feed(id: Int64) -> Feed? {
        feeds.first { $0.id == id }
    }

    /// Opens an article for reading (Rust marks it read). Nil, with the
    /// alert set, if it can't be opened.
    public func open(articleID: Int64) async -> ArticleText? {
        do {
            let text = try await core.openArticle(articleId: articleID)
            reload()
            return text
        } catch {
            log.warn("opening an article failed: \(error)")
            alert = .failure(error)
            return nil
        }
    }

    // MARK: Subscriptions

    /// Subscribes to the feed at `url`. Throws Rust's `AppError`, so the
    /// add sheet stays open with the address and shows why.
    public func add(url: String) async throws -> Feed {
        let cancellation = Cancellation()
        do {
            let feed = try await withTaskCancellationHandler {
                try await core.addFeed(url: url, cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            reload()
            return feed
        } catch {
            log.warn("adding a feed failed: \(error)")
            throw error
        }
    }

    public func remove(_ feed: Feed) async {
        await perform { try await $0.removeFeed(feedId: feed.id) }
    }

    public func rename(_ feed: Feed, to title: String) async {
        await perform { try await $0.renameFeed(feedId: feed.id, title: title) }
    }

    // MARK: Refresh

    /// Fetches every feed. Cancelling the calling task (pull to refresh
    /// let go, the screen gone) or `cancelRefresh()` stops it in Rust.
    public func refresh() async {
        guard !isRefreshing, !feeds.isEmpty else { return }
        isRefreshing = true
        let cancellation = Cancellation()
        refreshing = cancellation
        defer {
            isRefreshing = false
            refreshing = nil
        }
        do {
            let report = try await withTaskCancellationHandler {
                try await core.refresh(cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            lastRefresh = RefreshSummary(report: report, at: Date())
            reload()
        } catch {
            log.warn("refresh failed: \(error)")
            alert = .failure(error)
            reload()
        }
    }

    public func cancelRefresh() {
        refreshing?.cancel()
    }

    // MARK: Articles

    public func setRead(_ article: ArticleSummary, _ read: Bool) async {
        await perform { try await $0.setRead(articleId: article.id, read: read) }
    }

    public func setBookmarked(_ articleID: Int64, _ bookmarked: Bool) async {
        await perform { try await $0.setBookmarked(articleId: articleID, bookmarked: bookmarked) }
    }

    /// Marks a feed's articles read, or every feed's with nil.
    public func markAllRead(feedID: Int64?) async {
        await perform { _ = try await $0.markAllRead(feedId: feedID) }
    }

    // MARK: Private

    @discardableResult
    private func perform(_ command: (AppCore) async throws -> Void) async -> Bool {
        do {
            try await command(core)
            reload()
            return true
        } catch {
            log.warn("command failed: \(error)")
            alert = .failure(error)
            return false
        }
    }

    /// Words for the screen. `reason` fields are diagnostics from Rust:
    /// they go to the log where the error is caught, never on screen.
    nonisolated public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return error.localizedDescription }
        switch error {
        case .InvalidInput: return "That doesn't look like a web address. Check it and try again."
        case .NotFound: return "That's no longer in your library."
        case .AlreadySubscribed: return "You're already subscribed to this feed."
        case .NotAFeed: return "There's no RSS, Atom or JSON feed at that address."
        case .Http(let status): return "The site answered with an error (HTTP \(status))."
        case .Storage: return "Your library couldn't be read or saved. Try again, or restart the app."
        case .InUse: return "Your library is open in another copy of the app."
        case .Offline: return "You appear to be offline."
        case .Certificate: return "The site's security certificate was rejected."
        case .Timeout: return "The site took too long to answer."
        case .Network: return "Couldn't reach the site. Check the address and your connection, then try again."
        case .Cancelled: return "Cancelled."
        case .Interrupted: return "Stopped when the app went to the background. Try again."
        case .ShutDown: return "The app is closing."
        }
    }
}

/// What the last refresh did, in words for the status line.
public struct RefreshSummary: Equatable, Sendable {
    public let at: Date
    public let newArticles: UInt32
    public let failed: [String]

    public init(report: RefreshReport, at: Date) {
        self.at = at
        newArticles = report.newArticles
        failed = report.failures.map(\.title)
    }
}

extension AppError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Offline, .Timeout, .Network, .Interrupted, .Http: retryable = true
        default: retryable = false
        }
        return ForgeFailure(title: "Something Went Wrong", message: LibraryModel.describe(self), isRetryable: retryable)
    }

    public var isCancellation: Bool {
        if case .Cancelled = self { return true }
        return false
    }
}
