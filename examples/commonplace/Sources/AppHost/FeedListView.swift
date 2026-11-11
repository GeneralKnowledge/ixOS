@preconcurrency import ForgeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Screen 1: the smart timelines (unread, bookmarks, everything) and every
/// subscription with its unread count. Pull to refresh or the refresh
/// button fetches every feed; + adds one.
struct FeedListView: View {
    @Bindable var model: LibraryModel
    @State private var path: [Route] = []
    @State private var isAdding = false
    @State private var renaming: Feed?
    @State private var newTitle = ""
    @State private var unsubscribing: Feed?

    var body: some View {
        NavigationStack(path: $path) {
            List {
                Section {
                    smartRow(.unread, systemImage: "circle.inset.filled", tone: .accent)
                    smartRow(.bookmarked, systemImage: "bookmark.fill", tone: .warning)
                    smartRow(.all, systemImage: "tray.full.fill", tone: .neutral)
                }
                if !model.feeds.isEmpty {
                    Section("Feeds") {
                        ForEach(model.feeds) { feed in
                            NavigationLink(value: Route.timeline(.feed(feedId: feed.id))) {
                                FeedRow(feed: feed)
                            }
                            .accessibilityIdentifier("feeds.feed.\(feed.id)")
                            .swipeActions {
                                Button("Unsubscribe", systemImage: "trash", role: .destructive) {
                                    unsubscribing = feed
                                }
                                .accessibilityIdentifier("feeds.unsubscribe")
                            }
                            .contextMenu {
                                Button("Rename", systemImage: "pencil") {
                                    newTitle = feed.title
                                    renaming = feed
                                }
                                Button("Mark All as Read", systemImage: "checkmark.circle") {
                                    Task { await model.markAllRead(feedID: feed.id) }
                                }
                                Button("Unsubscribe", systemImage: "trash", role: .destructive) {
                                    unsubscribing = feed
                                }
                            }
                        }
                    }
                }
            }
            .overlay { emptyState }
            .refreshable { await model.refresh() }
            .navigationTitle("Commonplace")
            .navigationDestination(for: Route.self) { route in
                switch route {
                case .timeline(let timeline):
                    TimelineView(model: model, timeline: timeline)
                case .article(let id):
                    ArticleView(model: model, articleID: id)
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("Add Feed", systemImage: "plus") { isAdding = true }
                        .accessibilityIdentifier("feeds.add")
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
            .sheet(isPresented: $isAdding) {
                AddFeedView(model: model) { feed in
                    path.append(.timeline(.feed(feedId: feed.id)))
                }
            }
            .alert("Rename Feed", isPresented: renamingBinding) {
                TextField("Name", text: $newTitle)
                    .accessibilityIdentifier("feeds.rename.title")
                Button("Cancel", role: .cancel) {}
                    .accessibilityIdentifier("feeds.rename.cancel")
                Button("Rename") {
                    if let feed = renaming {
                        Task { await model.rename(feed, to: newTitle) }
                    }
                }
                .accessibilityIdentifier("feeds.rename.save")
            } message: {
                Text("Leave it empty to use the feed's own name.")
            }
            .confirmationDialog(
                "Unsubscribe from \(unsubscribing?.title ?? "this feed")?",
                isPresented: unsubscribingBinding, titleVisibility: .visible
            ) {
                Button("Unsubscribe", role: .destructive) {
                    if let feed = unsubscribing {
                        Task { await model.remove(feed) }
                    }
                }
                .accessibilityIdentifier("feeds.unsubscribe.confirm")
            } message: {
                Text("Its articles are removed from this device, bookmarked ones too.")
            }
            .forgeAlert($model.alert)
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
                description: "Add a site's RSS, Atom or JSON feed to read it here. Everything stays on this device.",
                action: ForgeStateAction("Add Feed", identifier: "feeds.addFirst") { isAdding = true }
            )
        }
    }

    private var renamingBinding: Binding<Bool> {
        Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })
    }

    private var unsubscribingBinding: Binding<Bool> {
        Binding(get: { unsubscribing != nil }, set: { if !$0 { unsubscribing = nil } })
    }
}

private struct FeedRow: View {
    let feed: Feed

    var body: some View {
        ForgeRow(
            feed.title,
            subtitle: feed.lastError == nil ? nil : "Couldn't refresh",
            systemImage: feed.lastError == nil ? "dot.radiowaves.up.forward" : "exclamationmark.triangle.fill",
            tone: feed.lastError == nil ? .accent : .danger
        ) {
            if feed.unreadCount > 0 {
                Text(feed.unreadCount, format: .number)
                    .accessibilityLabel("\(feed.unreadCount) unread")
            }
        }
    }
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
