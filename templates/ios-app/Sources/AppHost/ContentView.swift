@preconcurrency import ForgeFFI
import SwiftUI

/// The app's root. It owns the model, opens the Rust core once, and shows
/// the notes, a spinner while opening, or a recovery screen if opening failed.
public struct ContentView: View {
    @State private var model = NotesModel()

    public init() {}

    public var body: some View {
        Group {
            switch model.phase {
            case .loading:
                ProgressView()
            case .ready:
                NoteListView(model: model)
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't Open Notes", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try Again") { Task { await model.open() } }
                        .accessibilityIdentifier("content.retry")
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        // Opens Rust off the main thread. `open()` does nothing when already
        // open, so the view reappearing never opens a second instance.
        .task { await model.open() }
        // Reports the scene phase to Rust (see AppLifecycle.swift).
        .forgeLifecycle { model.setPhase($0) }
    }
}
