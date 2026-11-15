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
                    Toggle("Oldest First", isOn: setting(.oldestFirst))
                        .accessibilityIdentifier("settings.oldestFirst")
                    Toggle("Hide Read Articles", isOn: setting(.hideRead))
                        .accessibilityIdentifier("settings.hideRead")
                    Toggle("Show Summaries", isOn: setting(.showSummaries))
                        .accessibilityIdentifier("settings.showSummaries")
                }
                Section {
                    #if os(iOS)
                        Toggle("Open Links in Commonplace", isOn: setting(.openLinksInApp))
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
    private func setting(_ choice: Choice) -> Binding<Bool> {
        Binding(
            get: { choice.value(in: model.settings) },
            set: { on in Task { await model.update { choice.set(on, in: &$0) } } }
        )
    }

    private var alertsBinding: Binding<Bool> {
        Binding(
            get: { model.newArticleAlerts },
            set: { on in Task { await model.setNewArticleAlerts(on) } }
        )
    }
}

/// The settings the toggles change.
private enum Choice {
    case oldestFirst, hideRead, showSummaries, openLinksInApp

    func value(in settings: AppSettings) -> Bool {
        switch self {
        case .oldestFirst: return settings.oldestFirst
        case .hideRead: return settings.hideRead
        case .showSummaries: return settings.showSummaries
        case .openLinksInApp: return settings.openLinksInApp
        }
    }

    func set(_ on: Bool, in settings: inout AppSettings) {
        switch self {
        case .oldestFirst: settings.oldestFirst = on
        case .hideRead: settings.hideRead = on
        case .showSummaries: settings.showSummaries = on
        case .openLinksInApp: settings.openLinksInApp = on
        }
    }
}
