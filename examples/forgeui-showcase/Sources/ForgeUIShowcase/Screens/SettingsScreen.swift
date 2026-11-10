#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// A settings screen: the layout ForgeUI should make straightforward.
/// Every row is SwiftUI's own (`NavigationLink`, `Toggle`, `Picker`,
/// `LabeledContent`) with a `ForgeRow` as its label.
struct SettingsScreen: View {
    @Binding var theme: ShowcaseTheme
    @Binding var appearance: ShowcaseAppearance

    @State private var notifications = true
    @State private var confirmingSignOut = false
    @State private var signedOut = false

    init(theme: Binding<ShowcaseTheme>, appearance: Binding<ShowcaseAppearance>) {
        _theme = theme
        _appearance = appearance
    }

    var body: some View {
        List {
            Section("Account") {
                NavigationLink {
                    ProfileScreen()
                } label: {
                    ForgeRow("Your profile", subtitle: "Personal information", systemImage: "person.fill", tone: .accent)
                }
                .accessibilityIdentifier("settings.profile")
                Toggle(isOn: $notifications) {
                    ForgeRow("Notifications", subtitle: notifications ? "Enabled" : "Off", systemImage: "bell.badge.fill", tone: .danger)
                }
                .accessibilityIdentifier("settings.notifications")
                NavigationLink {
                    PlaceholderScreen(title: "Privacy", systemImage: "hand.raised.fill")
                } label: {
                    ForgeRow("Privacy", systemImage: "hand.raised.fill", tone: .info)
                }
                .accessibilityIdentifier("settings.privacy")
            }

            Section("Appearance") {
                Picker(selection: $appearance) {
                    ForEach(ShowcaseAppearance.allCases) { Text($0.title).tag($0) }
                } label: {
                    ForgeRow("Dark mode", systemImage: "moon.fill", tone: .neutral)
                }
                .accessibilityIdentifier("settings.darkMode")
                Picker(selection: $theme) {
                    ForEach(ShowcaseTheme.allCases) { Text($0.title).tag($0) }
                } label: {
                    ForgeRow("Accent colour", systemImage: "paintpalette.fill", tone: .accent)
                }
                .accessibilityIdentifier("settings.accent")
            }

            Section("About") {
                ForgeRow("Version", systemImage: "info.circle.fill", tone: .neutral, value: "1.0.0")
                    .accessibilityIdentifier("settings.version")
            }

            Section {
                Button(signedOut ? "Signed Out" : "Sign Out", role: .destructive) {
                    confirmingSignOut = true
                }
                .disabled(signedOut)
                .frame(maxWidth: .infinity)
                .accessibilityIdentifier("settings.signOut")
            }
        }
        .forgeGroupedBackground()
        .navigationTitle("Settings")
        .confirmationDialog("Sign out of this iPhone?", isPresented: $confirmingSignOut, titleVisibility: .visible) {
            Button("Sign Out", role: .destructive) { signedOut = true }
                .accessibilityIdentifier("settings.signOut.confirm")
        } message: {
            Text("Your tasks stay on this iPhone.")
        }
    }
}

private struct ProfileScreen: View {
    @State private var name = "Mia Rossi"
    @State private var email = "mia@example.com"

    var body: some View {
        List {
            Section {
                ForgeDetailHeader(name, subtitle: email, systemImage: "person.fill") {
                    ForgeStatus("Verified", tone: .success)
                }
                .listRowBackground(Color.clear)
            }
            Section("Details") {
                ForgeField("Name") {
                    TextField("Name", text: $name)
                        #if os(iOS)
                        .textContentType(.name)
                        #endif
                        .accessibilityIdentifier("profile.name")
                }
                ForgeField("Email", message: email.contains("@") ? nil : .error("Enter an email address, like name@example.com.")) {
                    TextField("Email", text: $email)
                        #if os(iOS)
                        .textContentType(.emailAddress)
                        #endif
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("profile.email")
                }
            }
        }
        .forgeGroupedBackground()
        .navigationTitle("Your profile")
    }
}

struct PlaceholderScreen: View {
    let title: String
    let systemImage: String

    var body: some View {
        ForgeStateView.empty("Nothing Here Yet", systemImage: systemImage, description: "The showcase stops at this screen.")
            .navigationTitle(title)
    }
}
#endif
