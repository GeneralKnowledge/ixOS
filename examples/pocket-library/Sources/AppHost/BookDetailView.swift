@preconcurrency import ForgeFFI
import SwiftUI

/// One book: progress, status, rating, notes, history; edit, share, delete.
struct BookDetailView: View {
    let bookID: UInt64
    @Environment(LibraryModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var pageText = ""
    @State private var editing = false
    @State private var confirmDelete = false
    @State private var error: PresentedError?
    @State private var finishedCount = 0
    @FocusState private var pageFieldFocused: Bool

    /// Read from the model each time, so edits elsewhere show at once.
    private var book: Book? {
        _ = model.books // re-render when the library changes
        return model.book(id: bookID)
    }

    var body: some View {
        Group {
            if let book {
                details(book)
            } else {
                ContentUnavailableView("Book Deleted", systemImage: "book.closed")
            }
        }
        .navigationTitle(book?.title ?? "")
        .inlineNavigationTitle()
        .navigationDestination(for: ReaderRoute.self) { route in
            ReaderView(bookID: route.bookID)
        }
        .errorAlert($error)
        .sensoryFeedback(.success, trigger: finishedCount)
    }

    private func details(_ book: Book) -> some View {
        List {
            Section {
                header(book)
            }
            readSection(book)
            Section("Progress") {
                progress(book)
            }
            Section("Status") {
                Picker("Status", selection: Binding(
                    get: { book.status },
                    set: { status in run { try await model.setStatus(book.id, status) } }
                )) {
                    ForEach(ReadingStatus.allCases) { status in
                        Label(status.label, systemImage: status.symbol).tag(status)
                    }
                }
                LabeledContent("Added", value: Dates.short(book.addedAtMs))
                if let started = book.startedAtMs {
                    LabeledContent("Started", value: Dates.short(started))
                }
                if let finished = book.finishedAtMs {
                    LabeledContent("Finished", value: Dates.short(finished))
                }
            }
            Section("Rating") {
                StarRating(rating: book.rating) { stars in
                    var input = BookInput(book)
                    input.rating = stars
                    run { try await model.update(book.id, input) }
                }
            }
            if !book.description.isEmpty {
                Section("Description") {
                    Text(book.description)
                        .font(.callout)
                        .textSelection(.enabled)
                }
            }
            Section("Notes") {
                if book.notes.isEmpty {
                    Button("Add notes") { editing = true }
                } else {
                    Text(book.notes).textSelection(.enabled)
                }
            }
            let history = model.history(for: book)
            if !history.isEmpty {
                Section("History") {
                    ForEach(history, id: \.id) { entry in
                        ActivityRow(entry: entry, showTitle: false)
                    }
                }
            }
            Section {
                Button("Delete Book", role: .destructive) { confirmDelete = true }
            }
        }
        .groupedList()
        .scrollDismissesKeyboard(.interactively)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Edit") { editing = true }
            }
            ToolbarItem(placement: .secondaryAction) {
                ShareLink(item: book.shareText) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
            }
            #if os(iOS)
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("Done") { pageFieldFocused = false }
            }
            #endif
        }
        .sheet(isPresented: $editing) {
            BookFormView(mode: .edit(book))
        }
        .confirmationDialog("Delete \u{201C}\(book.title)\u{201D}?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete Book", role: .destructive) {
                Task {
                    do {
                        try await model.delete(book.id)
                        dismiss()
                    } catch {
                        self.error = PresentedError(error)
                    }
                }
            }
        } message: {
            Text("Your notes and progress for this book are deleted. Its reading history stays.")
        }
        .onAppear { pageText = String(book.currentPage) }
        // The number pad has no Return key: save when the field loses focus
        // (keyboard Done, scrolling the keyboard away, or tapping elsewhere).
        .onChange(of: pageFieldFocused) { _, focused in
            if !focused { savePage(book) }
        }
        .onChange(of: book.currentPage) { _, page in pageText = String(page) }
    }

    /// Read or Continue Reading for a book with a saved EPUB; for an
    /// imported book from before EPUBs were kept, how to make it readable.
    @ViewBuilder
    private func readSection(_ book: Book) -> some View {
        if book.canRead {
            Section {
                NavigationLink(value: ReaderRoute(bookID: book.id)) {
                    Label(book.readingPosition == nil ? "Read" : "Continue Reading", systemImage: "book")
                        .font(.headline)
                }
            }
        } else if case .epub = book.source {
            Section {
                Label("Import this book's EPUB again to read it here. Your progress is kept.", systemImage: "info.circle")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func header(_ book: Book) -> some View {
        HStack(alignment: .top, spacing: 16) {
            CoverImage(title: book.title, file: book.coverFile, url: book.coverUrl, width: 96)
            VStack(alignment: .leading, spacing: 6) {
                Text(book.title)
                    .font(.title3.weight(.semibold))
                    .fixedSize(horizontal: false, vertical: true)
                Text(book.displayAuthor)
                    .foregroundStyle(.secondary)
                Label(book.status.label, systemImage: book.status.symbol)
                    .font(.subheadline)
                    .foregroundStyle(book.status.tint)
                if case .epub(let fileName) = book.source {
                    Label(fileName, systemImage: "doc")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                } else if case .openLibrary = book.source {
                    Label("Open Library", systemImage: "globe")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func progress(_ book: Book) -> some View {
        if let total = book.totalPages {
            VStack(alignment: .leading, spacing: 6) {
                ProgressView(value: book.progress)
                    .tint(book.status.tint)
                Text("\(Int((book.progress * 100).rounded()))% · page \(book.currentPage) of \(total)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        HStack {
            Text("Current page")
            Spacer()
            TextField("Page", text: $pageText)
                .numberEntry()
                .multilineTextAlignment(.trailing)
                .frame(maxWidth: 80)
                .focused($pageFieldFocused)
                .onSubmit { savePage(book) }
            Stepper("Page", value: Binding(
                get: { Int(book.currentPage) },
                set: { savePage(book, page: $0) }
            ), in: 0...Int(book.totalPages ?? 100_000))
            .labelsHidden()
        }
        if book.status != .finished {
            Button {
                run { try await model.setStatus(book.id, .finished) }
            } label: {
                Label("Mark as Finished", systemImage: "checkmark.circle")
            }
        }
    }

    private func savePage(_ book: Book, page: Int? = nil) {
        let value = page ?? Int(pageText.trimmingCharacters(in: .whitespaces)) ?? -1
        guard value >= 0, let page = UInt32(exactly: value) else {
            error = PresentedError(LibraryError.InvalidInput(reason: "enter a page number"))
            pageText = String(book.currentPage)
            return
        }
        guard page != book.currentPage else { return }
        run { try await model.updateProgress(book.id, page: page) }
    }

    /// Runs a command; shows its error here, and a haptic when a book is
    /// finished.
    private func run(_ command: @escaping @MainActor () async throws -> Book) {
        let wasFinished = book?.status == .finished
        Task {
            do {
                let updated = try await command()
                if !wasFinished, updated.status == .finished {
                    finishedCount += 1
                }
            } catch {
                self.error = PresentedError(error)
                if let book { pageText = String(book.currentPage) }
            }
        }
    }
}

/// 1–5 stars; tapping the current rating clears it. VoiceOver reads it as one
/// adjustable control ("Rating, 3 of 5 stars"; swipe up or down to change).
struct StarRating: View {
    let rating: UInt8?
    let onChange: (UInt8?) -> Void

    var body: some View {
        HStack(spacing: 10) {
            ForEach(1...5, id: \.self) { star in
                let filled = Int(rating ?? 0) >= star
                Button {
                    onChange(Int(rating ?? 0) == star ? nil : UInt8(star))
                } label: {
                    Image(systemName: filled ? "star.fill" : "star")
                        .font(.title3)
                        .foregroundStyle(filled ? .yellow : .secondary)
                }
                .buttonStyle(.borderless)
            }
            Spacer()
            if rating == nil {
                Text("Not rated").font(.caption).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Rating")
        .accessibilityValue(rating.map { "\($0) of 5 stars" } ?? "Not rated")
        .accessibilityAdjustableAction { direction in
            let current = Int(rating ?? 0)
            switch direction {
            case .increment where current < 5: onChange(UInt8(current + 1))
            case .decrement where current > 1: onChange(UInt8(current - 1))
            case .decrement where current == 1: onChange(nil)
            default: break
            }
        }
    }
}

extension BookInput {
    /// The edit form's starting values.
    init(_ book: Book) {
        self.init(
            title: book.title,
            author: book.author,
            description: book.description,
            status: book.status,
            currentPage: book.currentPage,
            totalPages: book.totalPages,
            rating: book.rating,
            notes: book.notes,
            coverUrl: book.coverUrl
        )
    }
}
