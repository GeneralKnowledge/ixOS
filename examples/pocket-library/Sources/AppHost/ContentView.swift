@preconcurrency import ForgeFFI
import SwiftUI

/// The app's root: three tabs over one library.
public struct ContentView: View {
    @State private var model = LibraryModel()
    @Environment(\.scenePhase) private var scenePhase

    public init() {}

    public var body: some View {
        Group {
            switch model.phase {
            case .opening:
                ProgressView()
            case .failed(let message, let canStartFresh):
                OpenFailedView(message: message, canStartFresh: canStartFresh, model: model)
            case .open:
                TabView {
                    LibraryView()
                        .tabItem { Label("Library", systemImage: "books.vertical") }
                    ActivityView()
                        .tabItem { Label("Activity", systemImage: "chart.bar") }
                    SettingsView()
                        .tabItem { Label("Settings", systemImage: "gearshape") }
                }
            }
        }
        .environment(model)
        .task { await model.open() }
        // One event loop for the app, started whenever the library opens.
        .task(id: model.phase == .open) {
            if model.phase == .open { await model.listenForEvents() }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { model.becameActive() }
        }
    }
}

/// Shown when the saved library cannot be read. Nothing is deleted without
/// the user saying so, and even then the old file is kept as a backup.
private struct OpenFailedView: View {
    let message: String
    let canStartFresh: Bool
    let model: LibraryModel
    @State private var confirmFresh = false

    var body: some View {
        ContentUnavailableView {
            Label("Can't Open Library", systemImage: "exclamationmark.triangle")
        } description: {
            Text(message)
        } actions: {
            Button("Try Again") { Task { await model.open() } }
                .buttonStyle(.borderedProminent)
            if canStartFresh {
                Button("Start a New Library…") { confirmFresh = true }
            }
        }
        .confirmationDialog(
            "Start a new, empty library?",
            isPresented: $confirmFresh,
            titleVisibility: .visible
        ) {
            Button("Start New Library", role: .destructive) {
                Task { await model.startFresh() }
            }
        } message: {
            Text("The unreadable library is kept as a backup file in the app's storage.")
        }
    }
}
