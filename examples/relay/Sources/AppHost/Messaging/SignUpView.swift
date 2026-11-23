@preconcurrency import RelayFFI
import ForgeAppKit
import SwiftUI

/// The chat servers the sign-up screen suggests.
enum ChatServers {
    /// The project's own server, on the maintainer's tailnet
    /// (examples/relay/messaging/chat-server/deploy): reachable from any device on that
    /// tailnet, anywhere, so Chat works in Device Lab without a laptop
    /// nearby. Nil until one is deployed.
    static let hosted: String? = nil
    /// A server on the computer next to you (`make chat-server`).
    static let local = "http://127.0.0.1:8787"
}

/// First launch: create an account on the chat server. Rust keeps the
/// account, token included, in the Keychain; the token never reaches Swift.
struct SignUpView: View {
    let model: ChatModel
    @AppStorage("chat.lastServerURL") private var serverURL = ChatServers.hosted ?? ChatServers.local
    @State private var displayName = ""
    @State private var isWorking = false
    @State private var error: ForgeFailure?
    @State private var showingAssistant = false

    private var canSubmit: Bool {
        !isWorking && !displayName.trimmingCharacters(in: .whitespaces).isEmpty
            && !serverURL.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Your name", text: $displayName)
                        .accessibilityIdentifier("signUp.name")
                        #if os(iOS)
                            .textContentType(.name)
                        #endif
                        .submitLabel(.go)
                        .onSubmit(submit)
                } header: {
                    Text("Name")
                } footer: {
                    Text("Shown next to your messages.")
                }
                Section {
                    TextField("http://192.168.1.10:8787", text: $serverURL)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("signUp.server")
                        #if os(iOS)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                        #endif
                    if let hosted = ChatServers.hosted, serverURL != hosted {
                        Button("Use the Project's Server") { serverURL = hosted }
                            .accessibilityIdentifier("signUp.useHosted")
                    }
                } header: {
                    Text("Chat server")
                } footer: {
                    if ChatServers.hosted != nil {
                        Text(
                            "The project's server works anywhere with Tailscale on. Or run `make chat-server ARGS=\"--bind 0.0.0.0:8787\"` on your computer and enter its address on your network."
                        )
                    } else {
                        Text(
                            "Run `make chat-server ARGS=\"--bind 0.0.0.0:8787\"` on your computer and enter its address on your network."
                        )
                    }
                }
                Section {
                    Button(action: submit) {
                        HStack {
                            Text("Create Account")
                            Spacer()
                            if isWorking { ProgressView() }
                        }
                    }
                    .disabled(!canSubmit)
                    .accessibilityIdentifier("signUp.create")
                }
            }
            .navigationTitle("Join a Chat Server")
            .toolbar {
                // The assistant runs on the phone and needs no account.
                if model.assistant != nil {
                    ToolbarItem(placement: .primaryAction) {
                        Button("Assistant", systemImage: "sparkles") { showingAssistant = true }
                            .accessibilityIdentifier("signUp.assistant")
                    }
                }
            }
            .sheet(isPresented: $showingAssistant) {
                if let assistant = model.assistant {
                    AssistantTab(model: assistant)
                }
            }
            .forgeAlert(failure: $error)
        }
    }

    private func submit() {
        guard canSubmit else { return }
        isWorking = true
        Task {
            defer { isWorking = false }
            do {
                try await model.signUp(serverURL: serverURL, displayName: displayName)
            } catch {
                self.error = ChatModel.failure(error, title: "Couldn't Create the Account")
            }
        }
    }
}

/// Joining a room host's rooms with a word code (N4): the account is this
/// device's key there, and everything goes over Relay's encrypted
/// connection. The host runs `chat-server invite` for the code.
struct JoinRoomView: View {
    let model: ChatModel
    @AppStorage("chat.lastServerURL") private var host = ""
    @State private var code = ""
    @State private var displayName = ""
    @State private var isWorking = false
    @State private var error: ForgeFailure?

    private var canSubmit: Bool {
        !isWorking && !displayName.trimmingCharacters(in: .whitespaces).isEmpty
            && code.split(whereSeparator: { $0 == "-" || $0 == " " }).count == 4
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("four words", text: $code)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("joinRoom.code")
                        #if os(iOS)
                            .textInputAutocapitalization(.never)
                        #endif
                } header: {
                    Text("Code")
                } footer: {
                    Text("Ask the room's host for a code. It works once, for ten minutes.")
                }
                Section {
                    TextField("Your name", text: $displayName)
                        .accessibilityIdentifier("joinRoom.name")
                        #if os(iOS)
                            .textContentType(.name)
                        #endif
                } header: {
                    Text("Name")
                } footer: {
                    Text("Shown next to your messages.")
                }
                Section {
                    TextField("http://192.168.1.10:8787", text: $host)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("joinRoom.host")
                        #if os(iOS)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                        #endif
                } header: {
                    Text("Host")
                } footer: {
                    Text(
                        "The room host's address, or its key. Messages go encrypted over Relay; the host passes them on and can read them."
                    )
                }
                Section {
                    Button(action: submit) {
                        HStack {
                            Text("Join")
                            Spacer()
                            if isWorking { ProgressView() }
                        }
                    }
                    .disabled(!canSubmit)
                    .accessibilityIdentifier("joinRoom.join")
                }
            }
            .navigationTitle("Join a Room")
            .forgeAlert(failure: $error)
        }
    }

    private func submit() {
        guard canSubmit else { return }
        isWorking = true
        Task {
            defer { isWorking = false }
            do {
                try await model.joinRoom(host: host, code: code, displayName: displayName)
            } catch {
                self.error = ChatModel.failure(error, title: "Couldn't Join")
            }
        }
    }
}
