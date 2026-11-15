@preconcurrency import CommonplaceFFI
import ForgeUI
import SwiftUI

/// Settings: how timelines list articles, where links open, the reader's
/// font and new-article notifications. Rust keeps the settings (the
/// reader's font is ForgeReader's own, kept on the device).
struct SettingsView: View {
    let model: LibraryModel
    @AppStorage(ForgeReaderSettings.serifKey) private var serif = true
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Timelines") {
                    Toggle("Oldest First", isOn: setting(\.oldestFirst))
                        .accessibilityIdentifier("settings.oldestFirst")
                    Toggle("Hide Read Articles", isOn: setting(\.hideRead))
                        .accessibilityIdentifier("settings.hideRead")
                    Toggle("Show Summaries", isOn: setting(\.showSummaries))
                        .accessibilityIdentifier("settings.showSummaries")
                }
                Section {
                    #if os(iOS)
                        Toggle("Open Links in Commonplace", isOn: setting(\.openLinksInApp))
                            .accessibilityIdentifier("settings.openLinksInApp")
                    #endif
                    Picker("Font", selection: $serif) {
                        Text("Serif").tag(true)
                        Text("System").tag(false)
                    }
                    .accessibilityIdentifier("settings.font")
                } header: {
                    Text("Reading")
                } footer: {
                    Text("Hide Read Articles applies to All Articles and each feed. Text size is in the reader's Text Settings menu.")
                }
                Section {
                    Toggle("Notify About New Articles", isOn: alertsBinding)
                        .accessibilityIdentifier("settings.newArticleAlerts")
                } footer: {
                    Text("When a refresh in the background finds new articles.")
                }
            }
            .navigationTitle("Settings")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("settings.done")
                }
            }
        }
    }

    /// A toggle on one of Rust's settings.
    private func setting(_ key: WritableKeyPath<AppSettings, Bool>) -> Binding<Bool> {
        Binding(
            get: { model.settings[keyPath: key] },
            set: { on in Task { await model.update { $0[keyPath: key] = on } } }
        )
    }

    private var alertsBinding: Binding<Bool> {
        Binding(
            get: { model.newArticleAlerts },
            set: { on in Task { await model.setNewArticleAlerts(on) } }
        )
    }
}
