@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// The Library tab: search, filter, sort, swipe actions, and the three ways
/// to add a book.
struct LibraryView: View {
    @Environment(LibraryModel.self) private var model
    @State private var importing = false
    @Environment(\.forgeTheme) private var theme

    private var bindable: Bindable<LibraryModel> { Bindable(model) }

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Library")
                // Pushed books are the model's state too (ForgeAppKit), so
                // "adding a book opens it" is checked on Linux.
                .forgeNavigationDestination(bindable.libraryDestination, \.book) { id in
                    BookDetailView(bookID: id)
                }
                // The system's own "Search" prompt: Apple's accessibility audit on
                // an iPhone found "Title or author" clipped in the search field.
                .searchable(text: bindable.searchText, prompt: "Search")
                .toolbar { toolbar }
        }
        .onAppear(perform: openReaderFromLaunchArgument)
        // Sheets, the file picker and alerts are the model's state
        // (ForgeAppKit), so the model tests can check them.
        .forgeSheet(bindable.libraryDestination, equals: .addBook) {
            BookFormView(mode: .add) { book in model.showBook(book.id) }
        }
        .forgeSheet(bindable.libraryDestination, equals: .catalogue) {
            CatalogueSearchView { book in model.showBook(book.id) }
        }
        .fileImporter(
            isPresented: bindable.libraryDestination.forgeIs(.importEpub),
            allowedContentTypes: [.epub]
        ) { result in
            switch result {
            case .success(let url): importEpub(url)
            case .failure(let failure): model.libraryAlert = .failure(failure)
            }
        }
        .forgeAlert(bindable.libraryAlert) { action in
            Task { await model.handle(action) }
        }
        .overlay {
            if importing {
                ProgressView("Importing…")
                    .padding()
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        if model.totalBooks == 0 {
            ForgeStateView.empty(
                "No Books Yet",
                systemImage: "books.vertical",
                description: "Add the books you're reading, have read, or want to read.",
                action: .menu("Add Book", systemImage: "plus", identifier: "library.empty.add") { addButtons }
            )
        } else if model.books.isEmpty {
            if model.searchText.isEmpty {
                ForgeStateView.empty(
                    "No \(model.statusFilter?.label ?? "") Books",
                    systemImage: model.statusFilter?.symbol ?? "book",
                    description: "Books you mark as \(model.statusFilter?.label ?? "") appear here."
                )
            } else {
                ForgeStateView.noResults(for: model.searchText)
            }
        } else {
            List {
                ForEach(model.books) { book in
                    NavigationRow { model.showBook(book.id) } label: {
                        BookRow(book: book)
                    }
                    .accessibilityIdentifier("library.book.\(book.id)")
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) { model.askToDelete(book) } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .swipeActions(edge: .leading) {
                        if book.status != .finished {
                            Button { setStatus(book, .finished) } label: {
                                Label("Finished", systemImage: "checkmark")
                            }
                            .tint(theme.colors.tone(ReadingStatus.finished.tone).solid)
                        }
                        if book.status == .wantToRead {
                            Button { setStatus(book, .reading) } label: {
                                Label("Start", systemImage: "book")
                            }
                            .tint(theme.colors.tone(ReadingStatus.reading.tone).solid)
                        }
                    }
                    .contextMenu {
                        ShareLink(item: book.shareText)
                        Menu("Mark as") {
                            ForEach(ReadingStatus.allCases) { status in
                                Button(status.label) { setStatus(book, status) }
                                    .disabled(status == book.status)
                            }
                        }
                        Button(role: .destructive) { model.askToDelete(book) } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                }
            }
            .groupedList()
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        // Declared first, so it sits to the left of Add, which takes the
        // trailing edge as the primary action (as in Reminders and Books).
        ToolbarItem(placement: .primaryAction) {
            // Filter and sort in one menu, as Mail and Files do.
            Menu {
                Picker("Show", selection: bindable.statusFilter) {
                    Label("All Books", systemImage: "books.vertical").tag(ReadingStatus?.none)
                    ForEach(ReadingStatus.allCases) { status in
                        Label(status.label, systemImage: status.symbol).tag(Optional(status))
                    }
                }
                Picker("Sort By", selection: bindable.sort) {
                    ForEach(LibrarySort.allCases) { order in
                        Text(order.label).tag(order)
                    }
                }
            } label: {
                Label(
                    "Filter and Sort",
                    systemImage: model.statusFilter == nil
                        ? "line.3.horizontal.decrease.circle"
                        : "line.3.horizontal.decrease.circle.fill"
                )
            }
            .accessibilityIdentifier("library.filter")
        }
        ToolbarItem(placement: .primaryAction) {
            addMenu(id: "library.add")
        }
    }

    /// The Add menu. It appears both in the toolbar and in the empty
    /// library, so each copy gets its own identifier.
    private func addMenu(id: String) -> some View {
        Menu {
            addButtons
        } label: {
            Label("Add Book", systemImage: "plus")
        }
        .accessibilityIdentifier(id)
    }

    /// The ways to add a book, for the toolbar's menu and the empty library's.
    @ViewBuilder
    private var addButtons: some View {
        Button { model.libraryDestination = .catalogue } label: {
            Label("Search Online Catalogue", systemImage: "magnifyingglass")
        }
        .disabled(!model.settings.catalogueEnabled)
        .accessibilityIdentifier("library.add.catalogue")
        Button { model.libraryDestination = .addBook } label: {
            Label("Enter Details", systemImage: "square.and.pencil")
        }
        .accessibilityIdentifier("library.add.manual")
        Button { model.libraryDestination = .importEpub } label: {
            Label("Import EPUB File", systemImage: "doc.badge.plus")
        }
        .accessibilityIdentifier("library.add.epub")
    }

    private func setStatus(_ book: Book, _ status: ReadingStatus) {
        Task {
            do { try await model.setStatus(book.id, status) } catch { model.libraryAlert = .failure(error) }
        }
    }

    /// `-openReader <id>` opens that book in the reader at launch: how the
    /// screenshot job shows the reader without UI automation.
    private func openReaderFromLaunchArgument() {
        let id = UserDefaults.standard.integer(forKey: "openReader")
        guard id > 0, model.libraryDestination == nil, model.book(id: UInt64(id))?.canRead == true else { return }
        model.showBook(UInt64(id))
        model.showReader(UInt64(id))
    }

    private func importEpub(_ url: URL) {
        importing = true
        Task {
            defer { importing = false }
            do {
                let book = try await model.importEpub(at: url)
                model.showBook(book.id)
            } catch {
                model.libraryAlert = .failure(error)
            }
        }
    }
}

struct BookRow: View {
    let book: Book
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        // At accessibility text sizes the cover goes above the text, so
        // titles get the row's full width instead of a narrow column.
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(spacing: 12))
        layout {
            CoverImage(title: book.title, file: book.coverFile, url: book.coverUrl, width: 44)
            VStack(alignment: .leading, spacing: 3) {
                Text(book.title)
                    .font(.headline)
                    .lineLimit(typeSize.isAccessibilitySize ? 4 : 2)
                Text(book.displayAuthor)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(typeSize.isAccessibilitySize ? 2 : 1)
                if typeSize.isAccessibilitySize {
                    // Full width: the summary on its own line, the bar below.
                    Label {
                        Text(book.pageSummary)
                    } icon: {
                        Image(systemName: book.status.symbol)
                            .foregroundStyle(theme.colors.tone(book.status.tone).solid)
                            .accessibilityHidden(true)
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    if book.status == .reading, book.totalPages != nil {
                        ProgressView(value: book.progress)
                            .progressViewStyle(.forge(tone: book.status.tone))
                    }
                } else {
                    HStack(spacing: 6) {
                        Image(systemName: book.status.symbol)
                            .foregroundStyle(theme.colors.tone(book.status.tone).solid)
                            .accessibilityHidden(true)
                        if book.status == .reading, book.totalPages != nil {
                            ProgressView(value: book.progress)
                                .progressViewStyle(.forge(tone: book.status.tone))
                                .frame(maxWidth: 90)
                        }
                        Text(book.pageSummary)
                            .monospacedDigit()
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
            if let rating = book.rating {
                Text(String(repeating: "★", count: Int(rating)))
                    .font(.caption2)
                    // Readable on white too; plain yellow was faint.
                    .forgeForeground(.warning)
                    .accessibilityLabel("\(rating) stars")
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }
}
