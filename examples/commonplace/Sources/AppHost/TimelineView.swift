@preconcurrency import CommonplaceFFI
import ForgeUI
import SwiftUI

/// Screen 2: one timeline's articles, newest first (or oldest, and without
/// read ones, as the settings say). Swipe right to mark
/// read or unread, left to bookmark; tapping opens the article.
struct TimelineView: View {
    let model: LibraryModel
    let timeline: Timeline
    /// Mark All as Read asks first: one stray tap could clear hundreds.
    @State private var confirmingMarkAll = false
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        let articles = model.timeline(timeline)
        List {
            ForEach(articles) { article in
                NavigationLink(value: Route.article(article.id)) {
                    ArticleRow(
                        article: article, showsFeed: timeline.feedID == nil,
                        showsSummary: model.settings.showSummaries)
                }
                .accessibilityIdentifier("timeline.article.\(article.id)")
                .swipeActions(edge: .leading) {
                    Button(
                        article.read ? "Mark as Unread" : "Mark as Read",
                        systemImage: article.read ? "circle.fill" : "checkmark.circle"
                    ) {
                        Task { await model.setRead(article, !article.read) }
                    }
                    .tint(theme.colors.accent.solid)
                    .accessibilityIdentifier("timeline.toggleRead")
                }
                .swipeActions(edge: .trailing) {
                    Button(
                        article.bookmarked ? "Remove Bookmark" : "Bookmark",
                        systemImage: article.bookmarked ? "bookmark.slash" : "bookmark"
                    ) {
                        Task { await model.setBookmarked(article.id, !article.bookmarked) }
                    }
                    .tint(.orange)
                    .accessibilityIdentifier("timeline.toggleBookmark")
                }
            }
        }
        .listStyle(.plain)
        .overlay {
            if articles.isEmpty { emptyState }
        }
        .refreshable { await model.refresh() }
        .navigationTitle(timeline.title(in: model))
        .toolbar {
            if timeline != .bookmarked {
                ToolbarItem(placement: .primaryAction) {
                    Button("Mark All as Read", systemImage: "checkmark.circle") {
                        confirmingMarkAll = true
                    }
                    .disabled(!articles.contains { !$0.read })
                    .accessibilityIdentifier("timeline.markAllRead")
                }
            }
        }
        .confirmationDialog(
            markAllTitle(unread: articles.filter { !$0.read }.count),
            isPresented: $confirmingMarkAll, titleVisibility: .visible
        ) {
            Button("Mark All as Read") {
                Task { await model.markAllRead(feedID: timeline.feedID) }
            }
            .accessibilityIdentifier("timeline.confirmMarkAllRead")
        }
    }

    private func markAllTitle(unread: Int) -> String {
        unread == 1 ? "Mark 1 article as read?" : "Mark \(unread) articles as read?"
    }

    @ViewBuilder private var emptyState: some View {
        switch timeline {
        case .unread:
            ForgeStateView.empty("All Caught Up", systemImage: "checkmark.circle",
                                 description: "There's nothing unread. Pull down to check for new articles.")
        case .bookmarked:
            ForgeStateView.empty("No Bookmarks", systemImage: "bookmark",
                                 description: "Bookmark an article to keep it here.")
        default:
            ForgeStateView.empty("No Articles", systemImage: "doc.text",
                                 description: "Pull down to check for new articles.")
        }
    }
}

/// A timeline or search result row.
struct ArticleRow: View {
    let article: ArticleSummary
    let showsFeed: Bool
    let showsSummary: Bool
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    init(article: ArticleSummary, showsFeed: Bool, showsSummary: Bool = true) {
        self.article = article
        self.showsFeed = showsFeed
        self.showsSummary = showsSummary
    }

    var body: some View {
        let large = dynamicTypeSize.isAccessibilitySize
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            // The unread dot; VoiceOver hears "Unread" in the label instead.
            Circle()
                .fill(article.read ? Color.clear : Color.accentColor)
                .frame(width: 8, height: 8)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(article.title)
                    .font(.headline)
                    .foregroundStyle(article.read ? .secondary : .primary)
                    .lineLimit(large ? 4 : 2)
                if showsSummary, !article.summary.isEmpty {
                    Text(article.summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(large ? 3 : 2)
                }
                HStack {
                    if showsFeed {
                        Text(article.feedTitle)
                    }
                    Text(article.published, format: .relative(presentation: .named))
                    if article.bookmarked {
                        Image(systemName: "bookmark.fill")
                            .accessibilityLabel("Bookmarked")
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityValue(article.read ? "" : "Unread")
    }
}
