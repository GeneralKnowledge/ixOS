@preconcurrency import CommonplaceFFI
import ForgeAppKit
import SwiftUI

/// The Add Feed sheet: a web address, checked and fetched by Rust, and
/// under it suggested feeds by category (rust/catalogue/suggested-feeds.json).
/// A suggestion is added only when its Add button is tapped, and the sheet
/// stays open so the person can pick several. On failure the sheet stays
/// open and says why.
struct AddFeedView: View {
    let model: LibraryModel
    /// Called with the new feed once it is saved (the list opens it).
    let added: (Feed) -> Void

    @State private var url = ""
    @State private var adding = ForgeSubmission()
    /// The suggestions being added now, by id: their rows show a spinner.
    @State private var addingSuggestions: Set<String> = []
    @FocusState private var isFocused: Bool
    @Environment(\.dismiss) private var dismiss

    private var trimmed: String {
        url.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Feed address", text: $url, prompt: Text("example.com/feed.xml"))
                        .focused($isFocused)
                        .autocorrectionDisabled()
                        #if os(iOS)
                            .textContentType(.URL)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                        #endif
                        .submitLabel(.done)
                        .onSubmit { Task { await add() } }
                        .accessibilityIdentifier("addFeed.url")
                } footer: {
                    Text("The address of an RSS, Atom or JSON feed. Commonplace fetches it now and keeps its articles on this device.")
                }
                ForEach(model.suggestedFeeds(), id: \.name) { category in
                    Section(category.name) {
                        ForEach(category.feeds, id: \.id) { feed in
                            suggestionRow(feed)
                        }
                    }
                }
            }
            .navigationTitle("Add Feed")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("addFeed.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    if adding.isWorking {
                        ProgressView()
                            .accessibilityLabel("Adding")
                    } else {
                        Button("Add") { Task { await add() } }
                            .disabled(trimmed.isEmpty)
                            .accessibilityIdentifier("addFeed.save")
                    }
                }
            }
            .interactiveDismissDisabled(adding.isWorking)
            .forgeAlert(failure: $adding.failure)
            // Straight to typing when the person has feeds; a new library
            // starts with the suggestions in view, not under the keyboard.
            .onAppear { isFocused = !model.feeds.isEmpty }
        }
    }

    private func suggestionRow(_ feed: SuggestedFeed) -> some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(feed.title)
                Text(feed.about)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 8)
            if feed.subscribed {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.tint)
                    .accessibilityLabel("Subscribed to \(feed.title)")
                    .accessibilityIdentifier("addFeed.suggestion.\(feed.id).subscribed")
            } else if addingSuggestions.contains(feed.id) {
                ProgressView()
                    .accessibilityLabel("Adding \(feed.title)")
            } else {
                Button("Add") { Task { await add(feed) } }
                    .buttonStyle(.bordered)
                    .accessibilityLabel("Add \(feed.title)")
                    .accessibilityIdentifier("addFeed.suggestion.\(feed.id)")
            }
        }
    }

    private func add(_ feed: SuggestedFeed) async {
        guard !addingSuggestions.contains(feed.id) else { return }
        addingSuggestions.insert(feed.id)
        defer { addingSuggestions.remove(feed.id) }
        do {
            try await model.add(suggestion: feed)
        } catch {
            if !ForgeFailure.isCancellation(error) {
                adding.failure = ForgeFailure(error)
            }
        }
    }

    private func add() async {
        guard !trimmed.isEmpty, !adding.isWorking else { return }
        let address = trimmed
        var feed: Feed?
        if await adding.run({ feed = try await model.add(url: address) }) {
            dismiss()
            if let feed { added(feed) }
        }
    }
}
