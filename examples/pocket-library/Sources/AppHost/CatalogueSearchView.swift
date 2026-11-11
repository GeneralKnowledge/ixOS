@preconcurrency import ForgeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Finds a book in the online catalogue (Open Library) and adds it. Every
/// failure leaves the library as it was and offers the manual form instead.
struct CatalogueSearchView: View {
    var onImported: (Book) -> Void

    @Environment(LibraryModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    /// The search being shown. Changing it (a new query, or a retry of the
    /// same one) cancels the previous `.task`.
    @State private var submitted: SearchRequest?
    /// The search's state as one value (ForgeAppKit).
    @State private var state: ForgeLoad<[CatalogueBook]> = .idle
    /// Counts searches, so a stopped one only tidies up after itself.
    @State private var searches = 0
    @State private var importingKey: String?
    @State private var importNotes: ImportNotes?
    @State private var error: ForgeAlert<Never>?
    @State private var showingManualForm = false
    /// Starts with the search field active and the keyboard up.
    @State private var searchActive = true

    struct SearchRequest: Equatable {
        let text: String
        let attempt: Int
    }

    struct ImportNotes: Identifiable {
        let id = UUID()
        let book: Book
        let warnings: [String]
    }

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Find a Book")
                .inlineNavigationTitle()
                .searchable(text: $query, isPresented: $searchActive, prompt: "Title, author or ISBN")
                .onSubmit(of: .search) { submit() }
                .task(id: submitted) { await search() }
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { dismiss() }
                            .accessibilityIdentifier("catalogue.cancel")
                    }
                }
        }
        .forgeAlert($error)
        .alert(
            "Added Without Some Details",
            isPresented: Binding(get: { importNotes != nil }, set: { if !$0 { finish(importNotes?.book) } }),
            presenting: importNotes
        ) { notes in
            Button("OK") { finish(notes.book) }
        } message: { notes in
            Text(notes.warnings.joined(separator: "\n"))
        }
        .sheet(isPresented: $showingManualForm) {
            BookFormView(mode: .add) { book in finish(book) }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch state {
        case .idle:
            ContentUnavailableView(
                "Search Open Library",
                systemImage: "magnifyingglass",
                description: Text("Find a book by title or author and add it with its cover and description.")
            )
        case .loading:
            ForgeStateView.loading("Searching…")
        case .failed(let failure):
            ContentUnavailableView {
                Label(failure.title, systemImage: "wifi.exclamationmark")
            } description: {
                Text(failure.message)
            } actions: {
                if failure.isRetryable {
                    Button("Try Again") { retry() }
                        .buttonStyle(.forgePrimary)
                        .accessibilityIdentifier("catalogue.retry")
                }
                Button("Enter Details by Hand") { showingManualForm = true }
                    .buttonStyle(.forgeTertiary)
                    .accessibilityIdentifier("catalogue.manual")
            }
        case .loaded(let results) where results.isEmpty:
            ContentUnavailableView {
                Label("No Results", systemImage: "book.closed")
            } description: {
                Text("Try other words, or enter the book's details yourself.")
            } actions: {
                Button("Enter Details by Hand") { showingManualForm = true }
                    .buttonStyle(.forgeTertiary)
                    .accessibilityIdentifier("catalogue.noResults.manual")
            }
        case .loaded(let results):
            List(results, id: \.key) { result in
                CatalogueRow(
                    result: result,
                    importing: importingKey == result.key,
                    disabled: importingKey != nil
                ) {
                    importBook(result)
                }
            }
            .listStyle(.plain)
            // Keeps the results on screen while it searches again: swapping
            // them for a spinner would remove the list that owns this
            // refresh, cancel it, and leave "Searching…" for good.
            .refreshable { await search(showingProgress: false) }
        }
    }

    private func submit() {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        submitted = SearchRequest(text: trimmed, attempt: (submitted?.attempt ?? 0) + 1)
    }

    private func retry() {
        guard let current = submitted else { return }
        submitted = SearchRequest(text: current.text, attempt: current.attempt + 1)
    }

    private func search(showingProgress: Bool = true) async {
        guard let submitted else { return }
        let before = state
        searches += 1
        let this = searches
        if showingProgress { state = .loading }
        // A newer search replaced this one, or the sheet closed: the model
        // has already stopped the Rust request. Put back what was showing,
        // so a stopped search never leaves the spinner behind.
        let outcome = await ForgeLoad.capture { try await model.searchCatalogue(submitted.text) }
        if case .idle = outcome {
            // Unless a newer search has taken over the screen.
            if this == searches, state.isLoading { state = before.isLoading ? .idle : before }
            return
        }
        guard !Task.isCancelled else { return }
        state = outcome
    }

    private func importBook(_ result: CatalogueBook) {
        if let id = result.libraryBookId, let book = model.book(id: id) {
            finish(book)
            return
        }
        importingKey = result.key
        Task {
            defer { importingKey = nil }
            do {
                let imported = try await model.importFromCatalogue(result.key)
                if imported.warnings.isEmpty {
                    finish(imported.book)
                } else {
                    importNotes = ImportNotes(book: imported.book, warnings: imported.warnings)
                }
            } catch {
                self.error = .failure(error)
            }
        }
    }

    private func finish(_ book: Book?) {
        importNotes = nil
        dismiss()
        if let book { onImported(book) }
    }
}

private struct CatalogueRow: View {
    let result: CatalogueBook
    let importing: Bool
    let disabled: Bool
    let add: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            CoverImage(title: result.title, file: nil, url: result.coverUrl, width: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(result.title)
                    .font(.headline)
                    .lineLimit(2)
                if !result.author.isEmpty {
                    Text(result.author)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Text(details)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            if importing {
                ProgressView()
            } else if result.libraryBookId != nil {
                Button(action: add) {
                    Label("In Library", systemImage: "checkmark.circle.fill")
                        .labelStyle(.iconOnly)
                        .foregroundStyle(.green)
                        .font(.title2)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Already in your library. Open it")
            } else {
                Button(action: add) {
                    Label("Add", systemImage: "plus.circle.fill")
                        .labelStyle(.iconOnly)
                        .font(.title2)
                }
                .buttonStyle(.borderless)
                .disabled(disabled)
                .accessibilityLabel("Add \(result.title)")
            }
        }
        .padding(.vertical, 2)
    }

    private var details: String {
        var parts: [String] = []
        if let year = result.firstPublished { parts.append(String(year)) }
        if let pages = result.totalPages { parts.append("\(pages) pages") }
        return parts.isEmpty ? " " : parts.joined(separator: " · ")
    }
}
