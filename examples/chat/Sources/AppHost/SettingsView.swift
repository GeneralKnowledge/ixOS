@preconcurrency import ForgeFFI
import SwiftUI

/// The account and the server, and signing out.
struct SettingsView: View {
    let model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var confirmingSignOut = false

    var body: some View {
        NavigationStack {
            Form {
                if let account = model.account {
                    Section("Account") {
                        LabeledContent("Name", value: account.displayName)
                        LabeledContent("Server", value: account.serverUrl)
                        LabeledContent("Account ID", value: account.userId)
                            .textSelection(.enabled)
                    }
                }
                if let problem = model.accountProblem {
                    Section {
                        Label(
                            "The server no longer accepts this account. Sign out and create a new one.",
                            systemImage: "person.crop.circle.badge.exclamationmark"
                        )
                        Text(problem).font(.footnote).foregroundStyle(.secondary)
                    }
                }
                Section {
                    Button("Sign Out", role: .destructive) { confirmingSignOut = true }
                } footer: {
                    Text(
                        "Removes the account from this iPhone's Keychain and its chats from this iPhone. Messages stay on the server."
                    )
                }
            }
            .navigationTitle("Settings")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .confirmationDialog(
                "Sign out of Chat?", isPresented: $confirmingSignOut, titleVisibility: .visible
            ) {
                Button("Sign Out", role: .destructive) {
                    dismiss()
                    model.signOut()
                }
            }
        }
    }
}
