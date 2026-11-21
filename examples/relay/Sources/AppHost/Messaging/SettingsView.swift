@preconcurrency import RelayFFI
import SwiftUI

/// The Settings tab: the account, the server, muted conversations, and
/// signing out.
struct SettingsView: View {
    let model: ChatModel
    /// Relay's core, for the pairing policy; `nil` while it opens.
    var relay: RelayCore? = nil
    @State private var confirmingSignOut = false
    @State private var joining = false

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
            if model.account == nil {
                Section {
                    Button("Join a Chat Server") { joining = true }
                        .accessibilityIdentifier("settings.joinServer")
                } header: {
                    Text("Chat Server")
                } footer: {
                    Text("Your trusted devices are in Chats already. A chat server adds conversations with other people.")
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
            #if os(iOS)
                if let relay {
                    RelaySettingsSection(core: relay)
                }
            #endif
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
                    "Muted conversations still show unread messages, without notifications on this iPhone. Notifications arrive while Relay is open or was used recently, and when iOS lets Relay check for news in the background: there is no push service, so they can be late."
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
                        ? "Set a passcode on this iPhone to lock Relay."
                        : "Relay asks when it opens and after a minute in the background, and hides its content in the app switcher."
                )
            }
            if model.account != nil {
                Section {
                    Button("Sign Out", role: .destructive) { confirmingSignOut = true }
                        .accessibilityIdentifier("settings.signOut")
                } footer: {
                    Text(
                        "Removes the account and its chats from this iPhone. The chat server has no sign-in, so you can't use this account again: you'd sign up as someone new. Your sent messages stay on the server."
                    )
                }
            }
        }
        .navigationTitle("Settings")
        .sheet(isPresented: $joining) {
            SignUpView(model: model)
        }
        .onChange(of: model.account != nil) { _, signedIn in
            if signedIn { joining = false }
        }
        .confirmationDialog(
            "Sign Out of the Chat Server?", isPresented: $confirmingSignOut, titleVisibility: .visible
        ) {
            Button("Sign Out", role: .destructive) { model.signOut() }
                .accessibilityIdentifier("settings.confirmSignOut")
        } message: {
            Text("You can't sign back in to this account later.")
        }
    }
}
