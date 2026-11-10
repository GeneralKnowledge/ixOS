@preconcurrency import ForgeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// One book: progress, status, rating, notes, history; edit, share, delete.
struct BookDetailView: View {
    enum DetailAction { case delete }

    let bookID: UInt64
    @Environment(LibraryModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var pageText = ""
    @State private var editing = false
    /// The delete confirmation, or an error (ForgeAppKit).
    @State private var alert: ForgeAlert<DetailAction>?
    @State private var finishedCount = 0
    @FocusState private var pageFieldFocused: Bool
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.forgeTheme) private var theme

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
        .forgeAlert($alert) { action in
            switch action {
            case .delete: deleteBook()
            }
        }
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
                .accessibilityIdentifier("bookDetail.status")
                LabeledContent("Added", value: Dates.short(book.addedAtMs))
                if let started = book.startedAtMs {
                    LabeledContent("Started", value: Dates.short(started))
                }
                if let finished = book.finishedAtMs {
                    LabeledContent("Finished", value: Dates.short(finished))
                }
            }
            Section("Rating") {
                StarRating(rating: book.rating, idPrefix: "bookDetail") { stars in
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
                        .accessibilityIdentifier("bookDetail.addNotes")
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
                Button("Delete Book", role: .destructive) {
                    alert = .confirm(
                        "Delete \u{201C}\(book.title)\u{201D}?",
                        message: "Your notes and progress for this book are deleted. Its reading history stays.",
                        confirm: "Delete Book",
                        action: .delete,
                        identifier: "bookDetail.delete",
                        destructive: true
                    )
                }
                    .accessibilityIdentifier("bookDetail.delete")
            }
        }
        .groupedList()
        .scrollDismissesKeyboard(.interactively)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Edit") { editing = true }
                    .accessibilityIdentifier("bookDetail.edit")
            }
            ToolbarItem(placement: .secondaryAction) {
                ShareLink(item: book.shareText) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .accessibilityIdentifier("bookDetail.share")
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
                .accessibilityIdentifier("bookDetail.read")
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
        // At accessibility text sizes the cover goes above the text, as in
        // the library list, so a long title isn't squeezed beside it.
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12))
            : AnyLayout(HStackLayout(alignment: .top, spacing: 16))
        return layout {
            CoverImage(title: book.title, file: book.coverFile, url: book.coverUrl, width: 96)
            VStack(alignment: .leading, spacing: 6) {
                Text(book.title)
                    .font(.title3.weight(.semibold))
                    .fixedSize(horizontal: false, vertical: true)
                Text(book.displayAuthor)
                    .foregroundStyle(.secondary)
                ForgeBadge(book.status.label, systemImage: book.status.symbol, tone: book.status.tone)
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
                    .tint(theme.colors.tone(book.status.tone).solid)
                Text("\(Int((book.progress * 100).rounded()))% · page \(book.currentPage) of \(total)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        HStack {
            Text("Current page")
                .accessibilityHidden(true)
            Spacer()
            // The visible "Current page" text isn't the field's label for
            // VoiceOver or UI tests, and a placeholder isn't either: name it.
            TextField("Page", text: $pageText)
                .numberEntry()
                .multilineTextAlignment(.trailing)
                .frame(maxWidth: 80)
                .focused($pageFieldFocused)
                .onSubmit { savePage(book) }
                .accessibilityLabel("Current page")
                .accessibilityIdentifier("bookDetail.page")
            Stepper("Current page", value: Binding(
                get: { Int(book.currentPage) },
                set: { savePage(book, page: $0) }
            ), in: 0...Int(book.totalPages ?? 100_000))
            .labelsHidden()
            .accessibilityIdentifier("bookDetail.pageStepper")
        }
        if book.status != .finished {
            Button {
                run { try await model.setStatus(book.id, .finished) }
            } label: {
                Label("Mark as Finished", systemImage: "checkmark.circle")
            }
            .accessibilityIdentifier("bookDetail.markFinished")
        }
    }

    private func deleteBook() {
        Task {
            do {
                try await model.delete(bookID)
                dismiss()
            } catch {
                alert = .failure(error)
            }
        }
    }

    private func savePage(_ book: Book, page: Int? = nil) {
        let value = page ?? Int(pageText.trimmingCharacters(in: .whitespaces)) ?? -1
        guard value >= 0, let page = UInt32(exactly: value) else {
            alert = .failure(ForgeFailure(title: "Check the Details", message: "Enter a page number."))
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
                alert = .failure(error)
                if let book { pageText = String(book.currentPage) }
            }
        }
    }
}

/// 1–5 stars; tapping the current rating clears it. VoiceOver reads it as one
/// adjustable control ("Rating, 3 of 5 stars"; swipe up or down to change),
/// identified as `<idPrefix>.rating`. UI automation can still see the stars
/// themselves, so each has its own name and identifier (`<idPrefix>.star.3`)
/// rather than five buttons all called `star`.
struct StarRating: View {
    let rating: UInt8?
    let idPrefix: String
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
                        // An SF Symbol's name becomes its identifier, and UI
                        // automation still sees the image inside the button.
                        .accessibilityIdentifier("\(idPrefix).star.\(star).image")
                        .accessibilityHidden(true)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel(star == 1 ? "1 star" : "\(star) stars")
                .accessibilityIdentifier("\(idPrefix).star.\(star)")
            }
            Spacer()
            if rating == nil {
                Text("Not rated").font(.caption).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Rating")
        .accessibilityIdentifier("\(idPrefix).rating")
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
