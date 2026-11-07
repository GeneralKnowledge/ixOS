@preconcurrency import ForgeFFI
import SwiftUI

/// The app's root. It owns the model, opens the Rust core once, and shows
/// sign-up, the conversations, a spinner, or a recovery screen.
public struct ContentView: View {
    @State private var model: ChatModel

    public init() {
        _model = State(initialValue: ChatModel(notifier: LocalChatNotifier.shared))
    }

    public var body: some View {
        Group {
            switch model.phase {
            case .loading:
                ProgressView()
            case .signedOut:
                SignUpView(model: model)
            case .ready:
                NavigationStack {
                    ConversationListView(model: model)
                }
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't Open Chat", systemImage: "exclamationmark.bubble")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try Again") { Task { await model.open() } }
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        // Opens Rust off the main thread; does nothing when already open.
        .task { await model.open() }
        // Reports the scene phase to Rust (see AppLifecycle.swift).
        .forgeLifecycle { model.setPhase($0) }
    }
}
