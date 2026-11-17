@preconcurrency import RelayFFI
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
                    "Muted conversations still show unread messages, without notifications on this iPhone. Notifications arrive while Chat is open or was used recently, and when iOS lets Chat check for news in the background: there is no push service, so they can be late."
                )
            }
            Section {
                Toggle(
                    "Require \(model.appLockMethod ?? "Passcode")",
                    isOn: Binding(get: { model.appLock }, set: { model.setAppLock($0) })
                )
                .disabled(model.appLockMethod == nil && !model.appLock)
                .accessibilityIdentifier("settings.appLock")
            } header: {
                Text("Privacy")
            } footer: {
                Text(
                    model.appLockMethod == nil
                        ? "Set a passcode on this iPhone to lock Chat."
                        : "Chat asks when it opens and after a minute in the background, and hides its content in the app switcher."
                )
            }
            Section {
                Button("Sign Out", role: .destructive) { confirmingSignOut = true }
                    .accessibilityIdentifier("settings.signOut")
            } footer: {
                Text(
                    "Removes the account and its chats from this iPhone. Chat has no sign-in, so you can't use this account again: you'd sign up as someone new. Your sent messages stay on the server."
                )
            }
        }
        .navigationTitle("Settings")
        .confirmationDialog(
            "Sign out of Chat?", isPresented: $confirmingSignOut, titleVisibility: .visible
        ) {
            Button("Sign Out", role: .destructive) { model.signOut() }
                .accessibilityIdentifier("settings.confirmSignOut")
        } message: {
            Text("You can't sign back in to this account later.")
        }
    }
}
