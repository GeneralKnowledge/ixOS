@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// Screen 3: one article. Opening it marks it read (Rust does). The toolbar
/// bookmarks it, marks it unread, opens the original in the browser and
/// shares its link.
struct ArticleView: View {
    let model: LibraryModel
    let articleID: Int64
    @State private var text: ArticleText?
    @State private var failed = false

    var body: some View {
        Group {
            if let text {
                ArticleReader(text: text)
            } else if failed {
                ForgeStateView.failed("Can't Open Article", message: "It may have been removed with its feed.")
            } else {
                ForgeStateView.loading()
            }
        }
        #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar { toolbar }
        .task(id: articleID) {
            text = await model.open(articleID: articleID)
            failed = text == nil
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        // The row's state, which Rust keeps current after each change.
        let summary = model.article(id: articleID)
        ToolbarItemGroup(placement: .primaryAction) {
            if let summary {
                Button(
                    summary.bookmarked ? "Remove Bookmark" : "Bookmark",
                    systemImage: summary.bookmarked ? "bookmark.fill" : "bookmark"
                ) {
                    Task { await model.setBookmarked(summary.id, !summary.bookmarked) }
                }
                .accessibilityIdentifier("article.bookmark")
                Menu("More", systemImage: "ellipsis.circle") {
                    Button(
                        summary.read ? "Mark as Unread" : "Mark as Read",
                        systemImage: summary.read ? "circle.fill" : "checkmark.circle"
                    ) {
                        Task { await model.setRead(summary, !summary.read) }
                    }
                    .accessibilityIdentifier("article.toggleRead")
                    if let link = summary.url.flatMap(URL.init(string:)) {
                        Link(destination: link) {
                            Label("Open in Browser", systemImage: "safari")
                        }
                        .accessibilityIdentifier("article.openInBrowser")
                        ShareLink(item: link)
                            .accessibilityIdentifier("article.share")
                    }
                }
                .accessibilityIdentifier("article.more")
            }
        }
    }
}

/// The reader's boundary: everything the reading surface shows comes in
/// `ArticleText`, and nothing else in the app depends on how it is drawn.
/// For now it is plain paragraphs; the native reader from the capability
/// roadmap (Tier 1) replaces this view without changing the rest.
struct ArticleReader: View {
    let text: ArticleText

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(text.feedTitle)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(text.title)
                        .font(.title2.bold())
                        .accessibilityAddTraits(.isHeader)
                        .accessibilityIdentifier("article.title")
                    HStack {
                        if let author = text.author {
                            Text(author)
                        }
                        Text(text.published, format: .dateTime.day().month().year().hour().minute())
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
                if text.paragraphs.isEmpty {
                    Text("This feed sends no text for the article. Open it in the browser to read it.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(Array(text.paragraphs.enumerated()), id: \.offset) { _, paragraph in
                        Text(paragraph)
                            .font(.body)
                            .textSelection(.enabled)
                    }
                }
            }
            .padding()
            .frame(maxWidth: 700, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .accessibilityIdentifier("article.body")
    }
}
