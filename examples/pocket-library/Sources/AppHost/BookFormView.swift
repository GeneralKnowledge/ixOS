@preconcurrency import ForgeFFI
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
    @State private var saving = false
    @State private var error: PresentedError?
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
                    TextField("Author", text: $input.author)
                        .wordsEntry()
                        .focused($focused, equals: .author)
                        .accessibilityLabel("Author")
                        .accessibilityIdentifier("bookForm.author")
                        .submitLabel(.next)
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
                    LabeledContent("Pages") {
                        TextField("Unknown", text: $totalPages)
                            .numberEntry()
                            .multilineTextAlignment(.trailing)
                            .focused($focused, equals: .totalPages)
                            .accessibilityIdentifier("bookForm.totalPages")
                    }
                }

                Section("Rating") {
                    StarRating(rating: input.rating, idPrefix: "bookForm") { input.rating = $0 }
                }

                Section("Notes") {
                    TextField("Thoughts, quotes, where you got it…", text: $input.notes, axis: .vertical)
                        .lineLimit(3...10)
                        .focused($focused, equals: .notes)
                        .accessibilityLabel("Notes")
                        .accessibilityIdentifier("bookForm.notes")
                }

                Section {
                    TextField("Description", text: $input.description, axis: .vertical)
                        .lineLimit(2...8)
                        .focused($focused, equals: .description)
                        .accessibilityLabel("Description")
                        .accessibilityIdentifier("bookForm.description")
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
                    if saving {
                        ProgressView()
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
            .interactiveDismissDisabled(saving || hasChanges)
            .errorAlert($error)
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

    private func save() {
        let numbers = [currentPage, totalPages].map { $0.trimmingCharacters(in: .whitespaces) }
        if numbers.contains(where: { !$0.isEmpty && UInt32($0) == nil }) {
            error = PresentedError(LibraryError.InvalidInput(reason: "page numbers must be whole numbers"))
            return
        }
        saving = true
        Task {
            defer { saving = false }
            do {
                let book: Book
                switch mode {
                case .add: book = try await model.add(assembled())
                case .edit(let original): book = try await model.update(original.id, assembled())
                }
                dismiss()
                onSaved(book)
            } catch {
                self.error = PresentedError(error)
            }
        }
    }
}
