import ChatBridge
import SwiftUI

/// The app's root: picks the server, then shows the conversation list.
///
/// The server URL is a setting because the Rust client talks to the demo
/// server directly: `http://127.0.0.1:8787` works when the app runs on the
/// same Mac as `make mock-chat`; on a phone, use the Mac's LAN address and
/// start the server with `ARGS="--bind 0.0.0.0:8787"`.
public struct ChatRootView: View {
    @AppStorage("chat.serverURL") private var serverURL = ChatRootView.defaultServerURL
    @State private var editingServer = false

    public static let defaultServerURL = "http://127.0.0.1:8787"

    public init() {}

    public var body: some View {
        SessionHost(serverURL: serverURL, editingServer: $editingServer)
            // A new URL means a new Rust client and store.
            .id(serverURL)
            .sheet(isPresented: $editingServer) {
                ServerSettingsView(serverURL: $serverURL)
            }
    }
}

/// Owns the session for one server URL.
private struct SessionHost: View {
    @StateObject private var session: SessionBox
    @Binding var editingServer: Bool

    init(serverURL: String, editingServer: Binding<Bool>) {
        _session = StateObject(wrappedValue: SessionBox(serverURL: serverURL))
        _editingServer = editingServer
    }

    var body: some View {
        NavigationStack {
            Group {
                switch session.state {
                case let .ready(chat):
                    ConversationListView(session: chat)
                case let .failed(message):
                    ContentUnavailableView(
                        "Can't open chat", systemImage: "exclamationmark.bubble",
                        description: Text(message))
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        editingServer = true
                    } label: {
                        Label("Server", systemImage: "server.rack")
                    }
                }
            }
        }
    }
}

/// Creating the Rust client can fail (bad URL, unreadable store), and
/// `@StateObject` needs a value either way.
@MainActor
private final class SessionBox: ObservableObject {
    enum State {
        case ready(ChatSession)
        case failed(String)
    }

    let state: State

    init(serverURL: String) {
        do {
            state = .ready(try ChatSession(serverURL: serverURL))
        } catch {
            state = .failed(ChatSession.message(for: error))
        }
    }
}

private struct ServerSettingsView: View {
    @Binding var serverURL: String
    @State private var draft = ""
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("http://192.168.1.10:8787", text: $draft)
                        .autocorrectionDisabled()
                        #if os(iOS)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                        #endif
                } header: {
                    Text("Chat server")
                } footer: {
                    Text(
                        "Run `make mock-chat` on your Mac. On a phone, start it with ARGS=\"--bind 0.0.0.0:8787\" and use the Mac's address."
                    )
                }
                Button("Use the default (\(ChatRootView.defaultServerURL))") {
                    draft = ChatRootView.defaultServerURL
                }
            }
            .navigationTitle("Server")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        serverURL = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                        dismiss()
                    }
                    .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear { draft = serverURL }
        }
    }
}
