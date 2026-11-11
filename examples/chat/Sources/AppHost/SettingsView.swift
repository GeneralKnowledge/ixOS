@preconcurrency import ChatFFI
import SwiftUI

/// The Settings tab: the account, the server, muted conversations, and
/// signing out.
struct SettingsView: View {
    let model: ChatModel
    @State private var confirmingSignOut = false

    private var muted: [ChatConversation] { model.sortedConversations.filter(\.muted) }

    var body: some View {
        Form {
            if let account = model.account {
                Section("Account") {
                    LabeledContent("Name", value: account.displayName)
                    LabeledContent("Server", value: account.serverUrl)
                    LabeledContent("Account ID", value: account.userId)
                        .textSelection(.enabled)
                    LabeledContent("Storage", value: storageEngine())
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
                if muted.isEmpty {
                    Text("None").foregroundStyle(.secondary)
                }
                ForEach(muted, id: \.id) { conversation in
                    Text(conversation.title)
                        .swipeActions {
                            Button("Unmute", systemImage: "bell") {
                                model.setMuted(conversation.id, false)
                            }
                        }
                }
            } header: {
                Text("Muted Conversations")
            } footer: {
                Text(
                    "Muted conversations still show unread messages, without notifications on this iPhone. Notifications arrive only while Chat is open or was used recently: there is no push service."
                )
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
        .confirmationDialog(
            "Sign out of Chat?", isPresented: $confirmingSignOut, titleVisibility: .visible
        ) {
            Button("Sign Out", role: .destructive) { model.signOut() }
        }
    }
}
