@preconcurrency import ForgeFFI
import ForgeAppKit
import SwiftUI

/// The Add Feed sheet: a web address, checked and fetched by Rust. On
/// failure the sheet stays open with the address and says why.
struct AddFeedView: View {
    let model: LibraryModel
    /// Called with the new feed once it is saved (the list opens it).
    let added: (Feed) -> Void

    @State private var url = ""
    @State private var adding = ForgeSubmission()
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
            .onAppear { isFocused = true }
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
