@preconcurrency import CommonplaceFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// Screen 1: the smart timelines (unread, bookmarks, everything), then the
/// folders and the feeds in none, with their unread counts. Pull to
/// refresh or the refresh button fetches every feed; + adds one; the
/// ellipsis menu makes a folder and imports or exports subscriptions as
/// OPML; the search field searches every saved article; the gear opens
/// Settings.
struct FeedListView: View {
    @Bindable var model: LibraryModel
    @State private var path: [Route] = []
    @State private var isAdding = false
    @State private var action: LibraryAction?
    @State private var choosingFile = false
    @State private var importDraft: OpmlDraft?
    @State private var importDone: OpmlImport?
    @State private var exported: URL?
    @State private var showingSettings = false
    @State private var query = ""
    @State private var results: [ArticleSummary] = []
    /// The search `results` answer: "No Results" waits for the search of
    /// what is typed now instead of flashing on every keystroke.
    @State private var resultsFor: SearchKey?

    var body: some View {
        NavigationStack(path: $path.forgeLayered()) {
            List {
                if !query.trimmingCharacters(in: .whitespaces).isEmpty {
                    searchResults
                } else {
                    lists
                }
            }
            .searchable(text: $query, prompt: "Search Articles")
            // A moment after typing stops; Rust reads the index directly.
            .task(id: SearchKey(query: query, version: model.version)) {
                do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
                let found = await model.search(query)
                guard !Task.isCancelled else { return }
                results = found
                resultsFor = SearchKey(query: query, version: model.version)
            }
            .overlay { emptyState }
            .refreshable { await model.refresh() }
            .task { await model.refreshWhenBackOnline() }
            // Fresh articles on launch, without a pull.
            .task { await model.refreshIfStale() }
            .navigationTitle("Commonplace")
            .forgeLayeredDestination(for: Route.self) { route in
                switch route {
                case .timeline(let timeline):
                    TimelineView(model: model, timeline: timeline)
                case .article(let id):
                    ArticleView(model: model, articleID: id)
                case .folder(let id):
                    FolderView(model: model, folderID: id)
                }
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Settings", systemImage: "gearshape") { showingSettings = true }
                        .accessibilityIdentifier("feeds.settings")
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Add Feed", systemImage: "plus") { isAdding = true }
                        .accessibilityIdentifier("feeds.add")
                }
                ToolbarItem(placement: .primaryAction) {
                    Menu("More", systemImage: "ellipsis.circle") {
                        Button("New Folder", systemImage: "folder.badge.plus") {
                            action = .newFolder(in: nil)
                        }
                        .accessibilityIdentifier("feeds.newFolder")
                        Button("Import Subscriptions…", systemImage: "square.and.arrow.down") {
                            choosingFile = true
                        }
                        .accessibilityIdentifier("feeds.import")
                        Button("Export Subscriptions", systemImage: "square.and.arrow.up") {
                            Task {
                                exported = await model.exportSubscriptions()
                            }
                        }
                        .disabled(model.feeds.isEmpty)
                        .accessibilityIdentifier("feeds.export")
                    }
                    .accessibilityIdentifier("feeds.more")
                }
                #if os(iOS)
                    ToolbarItemGroup(placement: .bottomBar) {
                        refreshButton
                        Spacer()
                        RefreshStatus(model: model)
                        Spacer()
                    }
                #else
                    ToolbarItem { refreshButton }
                #endif
            }
            .sheet(isPresented: $showingSettings) {
                SettingsView(model: model)
            }
            .sheet(isPresented: $isAdding) {
                AddFeedView(model: model) { feed in
                    path.forgeOpen(.timeline(.feed(feedId: feed.id)))
                }
            }
            .libraryActions($action, model: model)
            // OPML (and the XML it is): Feeder, NetNewsWire and most readers
            // export one of these.
            .fileImporter(
                isPresented: $choosingFile,
                allowedContentTypes: [UTType(filenameExtension: "opml") ?? .xml, .xml, .plainText]
            ) { result in
                switch result {
                case .success(let url):
                    importDraft = model.previewSubscriptions(at: url)
                case .failure(let error):
                    model.alert = .failure(error)
                }
            }
            .sheet(item: $importDraft) { draft in
                ImportSubscriptionsView(model: model, draft: draft) { importDone = $0 }
            }
            .forgeShareFile($exported)
            .alert(
                "Subscriptions Imported",
                isPresented: Binding(get: { importDone != nil }, set: { if !$0 { importDone = nil } })
            ) {
                Button("OK", role: .cancel) {}
                    .accessibilityIdentifier("import.done")
            } message: {
                if let done = importDone {
                    Text(Self.importSummary(done))
                }
            }
        }
        // On the stack, not its first screen: an article that fails to open
        // asks from a pushed screen, and an alert on the root showed behind it.
        // While Settings is open, it shows the alerts.
        .forgeAlert(Binding(
            get: { showingSettings ? nil : model.alert },
            set: { model.alert = $0 }
        ))
    }

    /// Matching articles, newest first, opening like timeline rows.
    @ViewBuilder private var searchResults: some View {
        if results.isEmpty {
            if resultsFor?.query == query {
                ForgeStateView.noResults(for: query)
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        } else {
            Section("^[\(results.count) article](inflect: true)") {
                ForEach(results) { article in
                    NavigationLink(value: Route.article(article.id)) {
                        ArticleRow(article: article, showsFeed: true, showsSummary: model.settings.showSummaries)
                    }
                    .accessibilityIdentifier("search.article.\(article.id)")
                }
            }
        }
    }

    @ViewBuilder private var lists: some View {
        Section {
            smartRow(.unread, systemImage: "circle.inset.filled", tone: .accent)
            smartRow(.bookmarked, systemImage: "bookmark.fill", tone: .warning)
            smartRow(.all, systemImage: "tray.full.fill", tone: .neutral)
        }
        if !model.feeds.isEmpty || !model.folders.isEmpty {
            Section("Feeds") {
                LibraryRows(model: model, folderID: nil, action: $action)
            }
        }
    }

    private func smartRow(_ timeline: Timeline, systemImage: String, tone: ForgeTone) -> some View {
        let counts = model.counts(timeline)
        // Bookmarks count what is kept; the others what is left to read.
        let shown = timeline == .bookmarked ? counts.total : counts.unread
        return NavigationLink(value: Route.timeline(timeline)) {
            ForgeRow(timeline.title(in: model), systemImage: systemImage, tone: tone) {
                if shown > 0 {
                    Text(shown, format: .number)
                }
            }
        }
        .accessibilityIdentifier("feeds.\(timeline.identifier)")
    }

    @ViewBuilder private var refreshButton: some View {
        if model.isRefreshing {
            Button("Stop Refreshing", systemImage: "xmark") { model.cancelRefresh() }
                .accessibilityIdentifier("feeds.stopRefresh")
        } else {
            Button("Refresh", systemImage: "arrow.clockwise") {
                Task { await model.refresh() }
            }
            .disabled(model.feeds.isEmpty)
            .accessibilityIdentifier("feeds.refresh")
        }
    }

    @ViewBuilder private var emptyState: some View {
        if model.feeds.isEmpty {
            ForgeStateView.empty(
                "No Feeds Yet", systemImage: "dot.radiowaves.up.forward",
                description: "Add a site's RSS, Atom or JSON feed, or pick from suggested ones, to read it here. Everything stays on this device.",
                action: ForgeStateAction("Add Feed", identifier: "feeds.addFirst") { isAdding = true }
            )
        }
    }

    /// "3 feeds added. 1 already subscribed. 2 folders made."
    static func importSummary(_ done: OpmlImport) -> String {
        var parts = [inflected("^[\(Int(done.added)) feed](inflect: true) added")]
        if done.alreadySubscribed > 0 {
            parts.append(String(localized: "\(Int(done.alreadySubscribed)) already subscribed, left as they were"))
        }
        if done.foldersCreated > 0 {
            parts.append(inflected("^[\(Int(done.foldersCreated)) folder](inflect: true) made"))
        }
        if done.filtersAdded > 0 {
            parts.append(inflected("^[\(Int(done.filtersAdded)) filter](inflect: true) kept"))
        }
        if done.skipped > 0 {
            parts.append(String(localized: "\(Int(done.skipped)) skipped"))
        }
        let text = parts.joined(separator: ". ") + "."
        return done.added > 0 ? text + " " + String(localized: "Their articles are being fetched.") : text
    }
}

/// Searches again when the words change or Rust's data does.
private struct SearchKey: Hashable {
    let query: String
    let version: Int
}

/// "Updated 10:42", "3 new articles", or "1 feed couldn't refresh".
private struct RefreshStatus: View {
    let model: LibraryModel

    var body: some View {
        Group {
            if model.isRefreshing {
                Text("Refreshing…")
            } else if let last = model.lastRefresh {
                if last.offline {
                    Text("Offline: feeds will refresh when you're connected")
                } else if !last.failed.isEmpty {
                    Text("^[\(last.failed.count) feed](inflect: true) couldn't refresh")
                } else if last.newArticles > 0 {
                    Text("^[\(Int(last.newArticles)) new article](inflect: true)")
                } else {
                    Text("Updated \(last.at, format: .dateTime.hour().minute())")
                }
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .accessibilityIdentifier("feeds.status")
    }
}
