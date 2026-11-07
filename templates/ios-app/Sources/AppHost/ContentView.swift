@preconcurrency import ForgeFFI
import SwiftUI

/// The starter screen: a notes list kept by Rust, and a URL check that runs
/// an HTTPS request from Rust. Replace it with your own UI.
public struct ContentView: View {
    public init() {}
    @StateObject private var model = NotesModel()

    public var body: some View {
        NavigationStack {
            Group {
                if model.isLoading {
                    ProgressView("Opening…")
                } else if let message = model.openError {
                    unavailable(message)
                } else {
                    notesList
                }
            }
            .navigationTitle("FORGE_APP_NAME")
        }
        // Opens Rust off the main thread. `open()` does nothing when already
        // open, so the view reappearing never opens a second instance.
        .task { await model.open() }
        // Reports the scene phase to Rust (see AppLifecycle.swift).
        .forgeLifecycle { model.setPhase($0) }
    }

    private var notesList: some View {
        List {
            Section {
                HStack {
                    TextField("New note", text: $model.draft)
                        .onSubmit { Task { await model.addNote() } }
                    Button("Add") { Task { await model.addNote() } }
                        .disabled(model.draft.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                if let message = model.errorMessage {
                    Text(message).font(.footnote).foregroundStyle(.red)
                }
            }
            Section("Notes") {
                if model.notes.isEmpty {
                    Text("No notes yet.").foregroundStyle(.secondary)
                }
                ForEach(model.notes, id: \.id) { note in
                    Text(note.text)
                }
                .onDelete { offsets in
                    let doomed = offsets.map { model.notes[$0] }
                    Task { for note in doomed { await model.delete(note) } }
                }
            }
            Section("Network check") {
                TextField("URL", text: $model.url)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                if model.isCheckingURL {
                    Button("Cancel", role: .cancel) { model.cancelURLCheck() }
                } else {
                    Button("Check") { Task { await model.checkURL() } }
                }
                if let result = model.urlResult {
                    Text(result).font(.footnote)
                }
            }
        }
    }

    private func unavailable(_ message: String) -> some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.largeTitle)
                .foregroundStyle(.orange)
            Text("Could not open your notes").font(.headline)
            Text(message).font(.footnote).multilineTextAlignment(.center)
            Button("Retry") { Task { await model.open() } }
                .buttonStyle(.borderedProminent)
        }
        .padding()
    }
}
