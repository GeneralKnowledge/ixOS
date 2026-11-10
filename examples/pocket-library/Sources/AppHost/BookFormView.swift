@preconcurrency import ForgeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The add and edit form. It only collects text: Rust validates it and
/// applies the reading rules when it is saved.
struct BookFormView: View {
    enum Mode {
        case add
        case edit(Book)
    }

    let mode: Mode
    var onSaved: (Book) -> Void = { _ in }

    @Environment(LibraryModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var input: BookInput
    @State private var currentPage: String
    @State private var totalPages: String
    @State private var coverURL: String
    /// The Save button's work and its failure (ForgeAppKit).
    @State private var saving = ForgeSubmission()
    /// Rust's validation, shown under each field it names.
    @State private var errors = ForgeFieldErrors()
    @FocusState private var focused: Field?

    private enum Field: Hashable { case title, author, currentPage, totalPages, notes, description, cover }

    init(mode: Mode, onSaved: @escaping (Book) -> Void = { _ in }) {
        self.mode = mode
        self.onSaved = onSaved
        let start: BookInput
        switch mode {
        case .add:
            start = BookInput(
                title: "", author: "", description: "", status: .wantToRead, currentPage: 0,
                totalPages: nil, rating: nil, notes: "", coverUrl: nil
            )
        case .edit(let book):
            start = BookInput(book)
        }
        _input = State(initialValue: start)
        _currentPage = State(initialValue: start.currentPage == 0 ? "" : String(start.currentPage))
        _totalPages = State(initialValue: start.totalPages.map(String.init) ?? "")
        _coverURL = State(initialValue: start.coverUrl ?? "")
    }

    private var isEditing: Bool {
        if case .edit = mode { return true }
        return false
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Title", text: $input.title, axis: .vertical)
                        .font(.headline)
                        .wordsEntry()
                        .focused($focused, equals: .title)
                        .accessibilityLabel("Title")
                        .accessibilityIdentifier("bookForm.title")
                        .submitLabel(.next)
                    fieldMessage("title")
                    TextField("Author", text: $input.author)
                        .wordsEntry()
                        .focused($focused, equals: .author)
                        .accessibilityLabel("Author")
                        .accessibilityIdentifier("bookForm.author")
                        .submitLabel(.next)
                    fieldMessage("author")
                }
                .onSubmit { advance() }

                Section("Reading") {
                    Picker("Status", selection: $input.status) {
                        ForEach(ReadingStatus.allCases) { status in
                            Text(status.label).tag(status)
                        }
                    }
                    .accessibilityIdentifier("bookForm.status")
                    // LabeledContent already names its field ("Current page");
                    // another accessibilityLabel would make VoiceOver say it twice.
                    LabeledContent("Current page") {
                        TextField("0", text: $currentPage)
                            .numberEntry()
                            .multilineTextAlignment(.trailing)
                            .focused($focused, equals: .currentPage)
                            .accessibilityIdentifier("bookForm.currentPage")
                    }
                    fieldMessage("current_page")
                    LabeledContent("Pages") {
                        TextField("Unknown", text: $totalPages)
                            .numberEntry()
                            .multilineTextAlignment(.trailing)
                            .focused($focused, equals: .totalPages)
                            .accessibilityIdentifier("bookForm.totalPages")
                    }
                    fieldMessage("total_pages")
                }

                Section("Rating") {
                    StarRating(rating: input.rating, idPrefix: "bookForm") { input.rating = $0 }
                    fieldMessage("rating")
                }

                Section("Notes") {
                    TextField("Thoughts, quotes, where you got it…", text: $input.notes, axis: .vertical)
                        .lineLimit(3...10)
                        .focused($focused, equals: .notes)
                        .accessibilityLabel("Notes")
                        .accessibilityIdentifier("bookForm.notes")
                    fieldMessage("notes")
                }

                Section {
                    TextField("Description", text: $input.description, axis: .vertical)
                        .lineLimit(2...8)
                        .focused($focused, equals: .description)
                        .accessibilityLabel("Description")
                        .accessibilityIdentifier("bookForm.description")
                    fieldMessage("description")
                    TextField("Cover image link (https://…)", text: $coverURL)
                        .autocorrectionDisabled()
                        .focused($focused, equals: .cover)
                        .accessibilityLabel("Cover image link")
                        .accessibilityIdentifier("bookForm.cover")
                        #if os(iOS)
                        .textContentType(.URL)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        #endif
                    fieldMessage("cover_url")
                } header: {
                    Text("Details")
                } footer: {
                    Text("Covers are shown when you're online. Books found in the catalogue or imported from EPUB keep a copy for offline use.")
                }
            }
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle(isEditing ? "Edit Book" : "New Book")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("bookForm.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    if saving.isWorking {
                        ProgressView()
                            .accessibilityLabel("Saving")
                    } else {
                        Button(isEditing ? "Save" : "Add") { save() }
                            .disabled(input.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                            .accessibilityIdentifier("bookForm.save")
                    }
                }
                #if os(iOS)
                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()
                    Button("Done") { focused = nil }
                }
                #endif
            }
            .interactiveDismissDisabled(saving.isWorking || hasChanges)
            .forgeAlert(failure: $saving.failure)
            .onAppear { if !isEditing { focused = .title } }
        }
    }

    private var hasChanges: Bool {
        switch mode {
        case .add: return !input.title.isEmpty || !input.author.isEmpty || !input.notes.isEmpty
        case .edit(let book): return assembled() != BookInput(book)
        }
    }

    private func advance() {
        switch focused {
        case .title: focused = .author
        case .author: focused = .currentPage
        default: focused = nil
        }
    }

    /// The form's text fields as a `BookInput`. Unparseable numbers are sent
    /// as they would be read, so Rust's validation explains the problem.
    private func assembled() -> BookInput {
        var result = input
        result.currentPage = UInt32(currentPage.trimmingCharacters(in: .whitespaces)) ?? 0
        let total = totalPages.trimmingCharacters(in: .whitespaces)
        result.totalPages = total.isEmpty ? nil : UInt32(total)
        let cover = coverURL.trimmingCharacters(in: .whitespaces)
        result.coverUrl = cover.isEmpty ? nil : cover
        return result
    }

    /// The field's problem from the last save, if any, under the field.
    @ViewBuilder
    private func fieldMessage(_ field: String) -> some View {
        if let message = errors[field] {
            ForgeFieldMessageView(message: message)
        }
    }

    private func save() {
        // Text that isn't a number never reaches Rust; say so where it is.
        var local = ForgeFieldErrors()
        for (field, text) in [("current_page", currentPage), ("total_pages", totalPages)] {
            let trimmed = text.trimmingCharacters(in: .whitespaces)
            if !trimmed.isEmpty && UInt32(trimmed) == nil {
                local.add(field, "Enter a whole number.")
            }
        }
        errors = local
        guard local.isEmpty else { return }
        let input = assembled()
        Task { @MainActor in
            var saved: Book?
            _ = await saving.run {
                do {
                    switch mode {
                    case .add: saved = try await model.add(input)
                    case .edit(let original): saved = try await model.update(original.id, input)
                    }
                } catch LibraryError.InvalidFields(let fields) {
                    // Shown under the fields, not as an alert.
                    errors = ForgeFieldErrors(fields)
                }
            }
            if let saved {
                dismiss()
                onSaved(saved)
            }
        }
    }
}
