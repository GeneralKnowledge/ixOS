@preconcurrency import CommonplaceFFI
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
    /// Every folder, as a tree: each followed by the folders inside it.
    public private(set) var folders: [Folder] = []
    /// Moves whenever Rust's data changed, so views that read timelines
    /// (`timeline(_:)` asks Rust each time) draw again.
    public private(set) var version = 0
    public private(set) var isRefreshing = false
    /// What the last refresh did, for the status line under the feeds.
    public private(set) var lastRefresh: RefreshSummary?
    /// A failed action, presented by the feed list with `.forgeAlert`.
    public var alert: ForgeAlert<Never>?
    /// A short message at the bottom of the screen: Undo after Mark All as
    /// Read (presented by the root with `.forgeBanner`).
    public var banner: ForgeBanner<ReadUndo>?
    /// Small haptics at the moments that deserve one (`.forgeFeedback`).
    public private(set) var feedback = ForgeFeedback()
    /// Whether a background refresh that finds new articles sends a
    /// notification (with a Mark All as Read button).
    public private(set) var newArticleAlerts = false
    /// The person's settings, as Rust keeps them.
    public private(set) var settings: AppSettings

    public let core: AppCore
    @ObservationIgnored private var refreshing: Cancellation?

    public init(core: AppCore) {
        self.core = core
        feeds = core.feeds()
        folders = core.folders()
        settings = core.settings()
        newArticleAlerts = core.newArticleAlerts()
    }

    /// Re-reads from Rust. Cheap: Rust hands over its published snapshot.
    public func reload() {
        feeds = core.feeds()
        folders = core.folders()
        settings = core.settings()
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

    public func folder(id: Int64) -> Folder? {
        folders.first { $0.id == id }
    }

    /// The feeds directly in a folder (at the top level with nil).
    public func feeds(in folderID: Int64?) -> [Feed] {
        feeds.filter { $0.folderId == folderID }
    }

    /// The folders directly in a folder (at the top level with nil).
    public func folders(in folderID: Int64?) -> [Folder] {
        folders.filter { $0.parentId == folderID }
    }

    /// Opens an article for reading (Rust marks it read). Nil, with the
    /// alert set, if it can't be opened.
    public func open(articleID: Int64) async -> ArticleText? {
        do {
            return try await openArticle(articleID)
        } catch {
            alert = .failure(error)
            return nil
        }
    }

    /// Opens an article for reading (Rust marks it read). Throws, so the
    /// article screen can say why where the person is, and offer Try Again.
    public func openArticle(_ articleID: Int64) async throws -> ArticleText {
        do {
            let text = try await core.openArticle(articleId: articleID)
            reload()
            return text
        } catch {
            log.warn("opening an article failed: \(error)")
            throw error
        }
    }

    /// The articles whose title or text holds every word of `query`, in
    /// timeline order. Empty for an empty query or if the search failed
    /// (logged: a search box shows no alert).
    public func search(_ query: String) async -> [ArticleSummary] {
        guard !query.trimmingCharacters(in: .whitespaces).isEmpty else { return [] }
        do {
            return try await core.search(query: query)
        } catch {
            log.warn("search failed: \(error)")
            return []
        }
    }

    // MARK: Settings

    /// Saves a change to the settings; the timelines follow at once.
    public func update(_ change: (inout AppSettings) -> Void) async {
        var changed = settings
        change(&changed)
        guard changed != settings else { return }
        settings = changed
        if !(await perform { try await $0.setSettings(settings: changed) }) {
            settings = core.settings()
        }
    }

    // MARK: Subscriptions

    /// Subscribes to the feed at `url`. Throws Rust's `AppError`, so the
    /// add sheet stays open with the address and shows why.
    public func add(url: String) async throws -> Feed {
        do {
            let feed = try await withRustCancellation { cancellation in
                try await core.addFeed(url: url, cancellation: cancellation)
            }
            reload()
            return feed
        } catch {
            log.warn("adding a feed failed: \(error)")
            throw error
        }
    }

    /// Popular feeds to start from, by category, each marked when it is
    /// already subscribed. Only offered: one is added when the person taps it.
    public func suggestedFeeds() -> [SuggestedCategory] {
        _ = version
        return core.suggestedFeeds()
    }

    /// Subscribes to a suggestion. Being subscribed already counts as done
    /// (its row only needed its check mark). Throws other failures, so the
    /// Add Feed sheet can say why where the person is.
    public func add(suggestion: SuggestedFeed) async throws {
        do {
            _ = try await add(url: suggestion.url)
        } catch AppError.AlreadySubscribed {
            reload()
        }
    }

    public func remove(_ feed: Feed) async {
        await perform { try await $0.removeFeed(feedId: feed.id) }
    }

    public func rename(_ feed: Feed, to title: String) async {
        await perform { try await $0.renameFeed(feedId: feed.id, title: title) }
    }

    /// Whether a feed's articles are fetched in full from their web pages.
    /// Turning it on fetches them with a refresh, started now.
    public func setFullText(_ feed: Feed, _ on: Bool) async {
        guard await perform({ try await $0.setFeedFullText(feedId: feed.id, on: on) }) else { return }
        if on { Task { await self.refresh() } }
    }

    /// Fetches one article's page now and returns the article with its
    /// readable text. Throws, so the article screen can say why there.
    public func loadFullArticle(_ articleID: Int64) async throws -> ArticleText {
        do {
            let text = try await withRustCancellation { cancellation in
                try await core.loadFullArticle(articleId: articleID, cancellation: cancellation)
            }
            reload()
            return text
        } catch {
            log.warn("loading a full article failed: \(error)")
            throw error
        }
    }

    /// Whether a feed's new articles count for the new-articles notification.
    public func setNotify(_ feed: Feed, _ notify: Bool) async {
        await perform { try await $0.setFeedNotify(feedId: feed.id, notify: notify) }
    }

    // MARK: Folders

    /// Makes a folder (or finds the one of that name already there).
    @discardableResult
    public func createFolder(named name: String, in parentID: Int64? = nil) async -> Folder? {
        var made: Folder?
        await perform { made = try await $0.createFolder(name: name, parentId: parentID) }
        return made
    }

    public func rename(_ folder: Folder, to name: String) async {
        await perform { try await $0.renameFolder(folderId: folder.id, name: name) }
    }

    /// Deletes a folder; its feeds and folders move up a level.
    public func delete(_ folder: Folder) async {
        await perform { try await $0.deleteFolder(folderId: folder.id) }
    }

    /// Puts a feed in a folder, or at the top level with nil.
    public func move(_ feed: Feed, to folderID: Int64?) async {
        await perform { try await $0.moveFeed(feedId: feed.id, folderId: folderID) }
    }

    /// Makes a folder and puts the feed in it.
    public func move(_ feed: Feed, toNewFolder name: String) async {
        guard let folder = await createFolder(named: name) else { return }
        await move(feed, to: folder.id)
    }

    // MARK: Subscriptions in and out (OPML)

    /// Reads an OPML file chosen in the document picker and says what
    /// importing it would do. Nothing changes until `importSubscriptions`.
    /// Nil, with the alert set, if it can't be read.
    public func previewSubscriptions(at url: URL) -> OpmlDraft? {
        #if canImport(Darwin)
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        #endif
        do {
            let data = try Data(contentsOf: url)
            let text = String(decoding: data, as: UTF8.self)
            return try previewSubscriptions(text: text)
        } catch {
            // Not the error itself: a parser message can quote the file.
            log.warn("reading an OPML file failed")
            alert = .failure(error)
            return nil
        }
    }

    /// The same, from the file's text.
    public func previewSubscriptions(text: String) throws -> OpmlDraft {
        OpmlDraft(contents: text, preview: try core.previewOpml(contents: text))
    }

    /// Imports the picked feeds of a previewed file, then fetches the new
    /// ones. Nil, with the alert set, on failure.
    public func importSubscriptions(_ draft: OpmlDraft, picked: Set<UInt32>) async -> OpmlImport? {
        var done: OpmlImport?
        let ok = await perform {
            done = try await $0.importOpml(contents: draft.contents, selected: picked.sorted())
        }
        guard ok, let done else { return nil }
        if done.added > 0 {
            feedback.play(.success)
            Task { await self.refresh() }
        }
        return done
    }

    /// Writes the subscriptions to an OPML file to share. Nil, with the
    /// alert set, on failure.
    public func exportSubscriptions() async -> URL? {
        do {
            let text = try await core.exportOpml()
            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent("Commonplace Subscriptions.opml")
            try Data(text.utf8).write(to: url, options: .atomic)
            return url
        } catch {
            log.warn("exporting subscriptions failed: \(error)")
            alert = .failure(error)
            return nil
        }
    }

    // MARK: Refresh

    /// Fetches every feed. Cancelling the calling task (pull to refresh
    /// let go, the screen gone) or `cancelRefresh()` stops it in Rust.
    public func refresh() async {
        guard !isRefreshing, !feeds.isEmpty else { return }
        isRefreshing = true
        defer {
            isRefreshing = false
            refreshing = nil
        }
        do {
            let report = try await withRustCancellation { cancellation in
                refreshing = cancellation
                return try await core.refresh(cancellation: cancellation)
            }
            lastRefresh = RefreshSummary(report: report, at: Date())
            reload()
        } catch AppError.Interrupted {
            // The background stopped it: no alert, it is tried again when
            // the app is back (`follow`), or now if it already is.
            log.info("refresh stopped by the background")
            refreshCutOff = true
            reload()
            if !isAway { Task { await self.cameBack() } }
        } catch {
            log.warn("refresh failed: \(error)")
            alert = .failure(error)
            reload()
        }
    }

    // MARK: The background

    /// Whether the app is in the background (`follow`).
    @ObservationIgnored private var isAway = false
    /// A refresh the background stopped, tried again on return.
    public private(set) var refreshCutOff = false

    /// Follows the app's phases through `host`, for as long as this model
    /// lives: each return from the background calls `cameBack`.
    public func follow(_ host: ForgeHost<AppCore>) {
        host.onBackground(self) { [weak self] in self?.isAway = true }
        host.onResume(self) { [weak self] _ in
            guard let self else { return }
            isAway = false
            Task { await self.cameBack() }
        }
    }

    /// Back from the background: a refresh the background stopped is tried
    /// again; otherwise one that found the device offline, or feeds older
    /// than `staleAfter`, are refreshed.
    public func cameBack() async {
        if refreshCutOff {
            refreshCutOff = false
            await refresh()
            return
        }
        await refreshIfOffline()
        await refreshIfStale()
    }

    /// How old the newest refresh may be before opening the app, or coming
    /// back to it, refreshes by itself.
    public static let staleAfter: TimeInterval = 30 * 60

    /// Refreshes when the feeds haven't been refreshed for `staleAfter`
    /// (or ever): on launch and when the app comes back, so the person
    /// doesn't open yesterday's articles and have to pull.
    public func refreshIfStale(now: Date = Date()) async {
        guard isStale(now: now) else { return }
        await refresh()
    }

    /// Whether `refreshIfStale` would refresh now.
    public func isStale(now: Date = Date()) -> Bool {
        guard !feeds.isEmpty else { return false }
        let newest = [lastRefresh?.at, feeds.compactMap(\.refreshedAt).max()].compactMap { $0 }.max()
        guard let newest else { return true }
        return now.timeIntervalSince(newest) >= Self.staleAfter
    }

    /// Refreshes again if the last refresh found the device offline. The
    /// view calls it when the network comes back and when the app returns
    /// to the foreground, which is what the offline footer promises.
    public func refreshIfOffline() async {
        guard lastRefresh?.offline == true else { return }
        await refresh()
    }

    /// Keeps the offline footer's promise while the feed list is open:
    /// each time the network comes back (Rust watches Forge's connectivity
    /// service), refreshes if the last refresh found the device offline.
    /// Returns when the task is cancelled, or at once where there is no
    /// connectivity service.
    public func refreshWhenBackOnline() async {
        while !Task.isCancelled {
            do {
                let back = try await withRustCancellation { cancellation in
                    try await core.waitUntilBackOnline(cancellation: cancellation)
                }
                guard back else { return }
            } catch {
                log.info("stopped waiting for the network: \(error)")
                return
            }
            await refreshIfOffline()
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

    /// Marks a feed's articles read, or every feed's with nil, at once,
    /// and offers Undo for a few seconds (a frequent, cheap action: undo
    /// rather than a confirmation, decided 2026-11-17).
    public func markAllRead(feedID: Int64?) async {
        await markRead { try await $0.markAllRead(feedId: feedID) }
    }

    /// Marks a folder's articles read (and its folders'), with Undo.
    public func markAllRead(folderID: Int64) async {
        await markRead { try await $0.markFolderRead(folderId: folderID) }
    }

    /// Marks what a timeline shows read: its feed, its folder, or every feed.
    public func markAllRead(in timeline: Timeline) async {
        if case .folder(let folderID) = timeline {
            await markAllRead(folderID: folderID)
        } else {
            await markAllRead(feedID: timeline.feedID)
        }
    }

    private func markRead(_ command: (AppCore) async throws -> MarkedRead) async {
        var marked: MarkedRead?
        let done = await perform { marked = try await command($0) }
        guard done, let marked, marked.count > 0 else { return }
        feedback.play(.success)
        let what = marked.count == 1 ? "1 article" : "\(marked.count) articles"
        banner = .undo("Marked \(what) as read", action: .markAllRead(token: marked.undoToken),
                       identifier: "timeline.undoMarkAllRead")
    }

    /// Takes back what a banner offered to undo. Rust leaves alone any
    /// article the person opened or marked in the meantime.
    public func undo(_ action: ReadUndo) async {
        switch action {
        case .markAllRead(let token):
            await perform { _ = try await $0.undoMarkAllRead(undoToken: token) }
        }
    }

    /// Turns the new-articles notification on (iOS asks for permission the
    /// first time) or off. Stays off if the person declined.
    public func setNewArticleAlerts(_ enabled: Bool) async {
        do {
            newArticleAlerts = try await core.setNewArticleAlerts(enabled: enabled)
        } catch {
            log.warn("changing new-article alerts failed: \(error)")
            alert = .failure(error)
        }
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
    /// A short alert title: the framework's word for the error's kind,
    /// except where Commonplace has a better one.
    nonisolated public static func title(_ error: AppError) -> String {
        switch error {
        case .AlreadySubscribed: return "Already Subscribed"
        case .NotAFeed: return "Not a Feed"
        case .NotOpml: return "Not a Subscription List"
        case .NoReadableText: return "No Article Found"
        case .Offline: return "You're Offline"
        case .Timeout: return "No Response"
        case .Http: return "The Site Had a Problem"
        case .Storage: return "Couldn't Save"
        default: return ForgeErrors.title(error.forgeKind)
        }
    }

    /// Words for why a feed's last refresh failed (`Feed.lastError` holds
    /// the kind Rust stored).
    nonisolated public static func refreshProblem(_ stored: String) -> String {
        switch stored {
        case "offline": return "Offline when it last refreshed"
        case "timeout": return "The site didn't answer"
        case "not-a-feed": return "No longer a feed at this address"
        case "certificate": return "The site's certificate was rejected"
        case "network": return "Couldn't reach the site"
        default:
            if stored.hasPrefix("http-") { return "The site answered with an error (HTTP \(stored.dropFirst(5)))" }
            return "Couldn't refresh"
        }
    }

    nonisolated public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return ForgeFailure(error).message }
        switch error {
        case .InvalidInput: return "That doesn't look like a web address. Check it and try again."
        case .NotFound: return "That's no longer in your library."
        case .NotOpml: return "That file isn't an OPML subscription list, or it's too large to be one."
        case .NoReadableText: return "The article's page has no text Commonplace can read. Open it in the browser instead."
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
    /// Every feed failed because the device is offline.
    public let offline: Bool

    public init(report: RefreshReport, at: Date) {
        self.at = at
        newArticles = report.newArticles
        failed = report.failures.map(\.title)
        offline =
            !report.failures.isEmpty
            && report.failures.allSatisfy {
                if case .Offline = $0.error { return true }
                return false
            }
    }
}

extension AppError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Offline, .Timeout, .Network, .Interrupted, .Http: retryable = true
        default: retryable = false
        }
        return ForgeFailure(title: LibraryModel.title(self), message: LibraryModel.describe(self), isRetryable: retryable)
    }

    public var isCancellation: Bool {
        if case .Cancelled = self { return true }
        return false
    }
}

/// An OPML file read but not imported yet: its text (imported as read)
/// and what importing it would do.
public struct OpmlDraft: Identifiable, Sendable {
    public let id = UUID()
    public let contents: String
    public let preview: OpmlPreview

    /// The feeds picked at first: the new ones.
    public var picked: Set<UInt32> {
        Set(preview.feeds.filter { $0.status == .new }.map(\.index))
    }
}

/// What the Undo banner can take back.
public enum ReadUndo: Equatable, Sendable {
    /// The Mark All as Read that Rust handed this token for.
    case markAllRead(token: UInt64)
}
