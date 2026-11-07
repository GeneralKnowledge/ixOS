@preconcurrency import ForgeFFI
import SwiftUI

/// First launch: create an account on the chat server. Rust keeps the
/// account, token included, in the Keychain; the token never reaches Swift.
struct SignUpView: View {
    let model: ChatModel
    @AppStorage("chat.lastServerURL") private var serverURL = "http://127.0.0.1:8787"
    @State private var displayName = ""
    @State private var isWorking = false
    @State private var error: String?

    private var canSubmit: Bool {
        !isWorking && !displayName.trimmingCharacters(in: .whitespaces).isEmpty
            && !serverURL.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Your name", text: $displayName)
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
                        #if os(iOS)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                        #endif
                } header: {
                    Text("Chat server")
                } footer: {
                    Text(
                        "Run `make chat-server ARGS=\"--bind 0.0.0.0:8787\"` on your computer and enter its address on your network."
                    )
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
                }
            }
            .navigationTitle("Welcome to Chat")
            .errorAlert("Couldn't Create the Account", message: $error)
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
                self.error = ChatModel.describe(error)
            }
        }
    }
}
