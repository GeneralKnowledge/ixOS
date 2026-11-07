@preconcurrency import ForgeFFI
import SwiftUI
import UniformTypeIdentifiers

/// The Library tab: search, filter, sort, swipe actions, and the three ways
/// to add a book.
struct LibraryView: View {
    @Environment(LibraryModel.self) private var model
    @State private var showingAddForm = false
    @State private var showingCatalogue = false
    @State private var showingImporter = false
    @State private var importing = false
    @State private var pendingDelete: Book?
    @State private var error: PresentedError?
    @State private var path = NavigationPath()

    private var bindable: Bindable<LibraryModel> { Bindable(model) }

    var body: some View {
        NavigationStack(path: $path) {
            content
                .navigationTitle("Library")
                .navigationDestination(for: UInt64.self) { id in
                    BookDetailView(bookID: id)
                }
                .searchable(text: bindable.searchText, prompt: "Title or author")
                .toolbar { toolbar }
        }
        .sheet(isPresented: $showingAddForm) {
            BookFormView(mode: .add) { book in path.append(book.id) }
        }
        .sheet(isPresented: $showingCatalogue) {
            CatalogueSearchView { book in path.append(book.id) }
        }
        .fileImporter(isPresented: $showingImporter, allowedContentTypes: [.epub]) { result in
            switch result {
            case .success(let url): importEpub(url)
            case .failure(let failure): error = PresentedError(failure)
            }
        }
        .confirmationDialog(
            "Delete \u{201C}\(pendingDelete?.title ?? "")\u{201D}?",
            isPresented: Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } }),
            titleVisibility: .visible,
            presenting: pendingDelete
        ) { book in
            Button("Delete Book", role: .destructive) { delete(book) }
        } message: { _ in
            Text("Your notes and progress for this book are deleted. Its reading history stays.")
        }
        .errorAlert($error)
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
            ContentUnavailableView {
                Label("No Books Yet", systemImage: "books.vertical")
            } description: {
                Text("Add the books you're reading, have read, or want to read.")
            } actions: {
                addMenu.buttonStyle(.borderedProminent)
            }
        } else if model.books.isEmpty {
            if model.searchText.isEmpty {
                ContentUnavailableView(
                    "No \(model.statusFilter?.label ?? "") Books",
                    systemImage: model.statusFilter?.symbol ?? "book",
                    description: Text("Books you mark as \(model.statusFilter?.label ?? "") appear here.")
                )
            } else {
                ContentUnavailableView.search(text: model.searchText)
            }
        } else {
            List {
                ForEach(model.books) { book in
                    NavigationLink(value: book.id) {
                        BookRow(book: book)
                    }
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) { pendingDelete = book } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .swipeActions(edge: .leading) {
                        if book.status != .finished {
                            Button { setStatus(book, .finished) } label: {
                                Label("Finished", systemImage: "checkmark")
                            }
                            .tint(.green)
                        }
                        if book.status == .wantToRead {
                            Button { setStatus(book, .reading) } label: {
                                Label("Start", systemImage: "book")
                            }
                            .tint(.blue)
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
                        Button(role: .destructive) { pendingDelete = book } label: {
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
        }
        ToolbarItem(placement: .primaryAction) {
            addMenu
        }
    }

    private var addMenu: some View {
        Menu {
            Button { showingCatalogue = true } label: {
                Label("Search Online Catalogue", systemImage: "magnifyingglass")
            }
            .disabled(!model.settings.catalogueEnabled)
            Button { showingAddForm = true } label: {
                Label("Enter Details", systemImage: "square.and.pencil")
            }
            Button { showingImporter = true } label: {
                Label("Import EPUB File", systemImage: "doc.badge.plus")
            }
        } label: {
            Label("Add Book", systemImage: "plus")
        }
    }

    private func setStatus(_ book: Book, _ status: ReadingStatus) {
        Task {
            do { try await model.setStatus(book.id, status) } catch { self.error = PresentedError(error) }
        }
    }

    private func delete(_ book: Book) {
        Task {
            do { try await model.delete(book.id) } catch { self.error = PresentedError(error) }
        }
    }

    private func importEpub(_ url: URL) {
        importing = true
        Task {
            defer { importing = false }
            do {
                let book = try await model.importEpub(at: url)
                path.append(book.id)
            } catch {
                self.error = PresentedError(error)
            }
        }
    }
}

struct BookRow: View {
    let book: Book
    @Environment(\.dynamicTypeSize) private var typeSize

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
                            .foregroundStyle(book.status.tint)
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    if book.status == .reading, book.totalPages != nil {
                        ProgressView(value: book.progress)
                            .tint(book.status.tint)
                    }
                } else {
                    HStack(spacing: 6) {
                        Image(systemName: book.status.symbol)
                            .foregroundStyle(book.status.tint)
                        if book.status == .reading, book.totalPages != nil {
                            ProgressView(value: book.progress)
                                .tint(book.status.tint)
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
                    .foregroundStyle(.yellow)
                    .accessibilityLabel("\(rating) stars")
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }
}
