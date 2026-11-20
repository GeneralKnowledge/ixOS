@preconcurrency import CommonplaceFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Screen 3: one article, on ForgeReader. Opening it marks it read (Rust
/// does). The toolbar sets the text size and font, bookmarks it, marks it
/// unread, loads the full article from its page, shows the original page
/// and shares its link.
struct ArticleView: View {
    let model: LibraryModel
    let articleID: Int64
    @State private var text: ForgeLoad<ArticleText> = .loading
    /// Moves to open the article again (Try Again).
    @State private var attempt = 0
    /// Loading the full article from its page (Load Full Article).
    @State private var loadingFull = ForgeSubmission()
    #if os(iOS)
        @State private var original: ForgeWebPage?
    #endif

    var body: some View {
        // A failure says why (Rust's error, in LibraryModel's words), with
        // Try Again when it might pass.
        ForgeLoadView(
            text,
            retry: ForgeStateAction("Try Again", identifier: "article.retry") { attempt += 1 }
        ) { text in
            ArticleReader(text: text, openLinksInApp: linksOpenInApp) { url in
                #if os(iOS)
                    original = .link(url)
                #endif
            }
        }
        .toolbar { toolbar }
        #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            // The article's own site opens in the sheet; links elsewhere go to Safari.
            .forgeWebSheet($original)
        #endif
        .forgeAlert(failure: $loadingFull.failure)
        .task(id: OpenRequest(articleID: articleID, attempt: attempt)) {
            text = text.whileReloading
            text = await text.reloaded { try await model.openArticle(articleID) }
        }
    }

    /// The web sheet is iOS's; elsewhere links go to the browser.
    private var linksOpenInApp: Bool {
        #if os(iOS)
            return model.settings.openLinksInApp
        #else
            return false
        #endif
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        // The row's state, which Rust keeps current after each change.
        let summary = model.article(id: articleID)
        ToolbarItemGroup(placement: .primaryAction) {
            ForgeReaderTextMenu(identifier: "article.textSettings")
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
                    if summary.url != nil, text.value?.full == false {
                        Button("Load Full Article", systemImage: "doc.richtext") {
                            Task { await loadFull() }
                        }
                        .disabled(loadingFull.isWorking)
                        .accessibilityIdentifier("article.loadFull")
                    }
                    if let link = summary.url.flatMap(URL.init(string:)) {
                        #if os(iOS)
                            Button("Show Original Page", systemImage: "globe") {
                                original = .link(link)
                            }
                            .accessibilityIdentifier("article.showOriginal")
                        #endif
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

extension ArticleView {
    /// Replaces the feed's text with the page's, or says why it couldn't.
    private func loadFull() async {
        var loaded: ArticleText?
        if await loadingFull.run({ loaded = try await model.loadFullArticle(articleID) }), let loaded {
            text = .loaded(loaded)
        }
    }
}

private struct OpenRequest: Hashable {
    let articleID: Int64
    let attempt: Int
}

/// The article on ForgeReader: its details above, then its body laid out
/// as blocks (headings, quotes, lists, code, pictures) that follow the
/// reader's text settings. Links open in the web sheet or in Safari, as
/// the settings say.
struct ArticleReader: View {
    let text: ArticleText
    let openLinksInApp: Bool
    let openInApp: (URL) -> Void
    @State private var topBlock: Int?

    var body: some View {
        ForgeReaderSectionView(
            blocks: text.body.blocks,
            topBlock: $topBlock,
            header: { header },
            footer: { footer }
        )
        .accessibilityIdentifier("article.body")
        .environment(\.openURL, OpenURLAction { url in
            guard openLinksInApp, let scheme = url.scheme?.lowercased(),
                  scheme == "https" || scheme == "http" else { return .systemAction }
            openInApp(url)
            return .handled
        })
    }

    private var header: some View {
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
        .padding(.bottom, 8)
    }

    @ViewBuilder private var footer: some View {
        if text.body.blocks.isEmpty {
            Text("This feed sends no text for the article. Open it in the browser to read it.")
                .foregroundStyle(.secondary)
        }
    }
}
