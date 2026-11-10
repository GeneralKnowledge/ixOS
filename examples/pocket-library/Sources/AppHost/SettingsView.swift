@preconcurrency import ForgeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The Settings tab: preferences, and the user's local data.
struct SettingsView: View {
    enum SettingsAction { case erase }

    @Environment(LibraryModel.self) private var model
    @State private var exportURL: URL?
    @State private var exporting = false
    @State private var storage: StorageInfo?
    /// The erase confirmation, or an error (ForgeAppKit).
    @State private var alert: ForgeAlert<SettingsAction>?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Sort library by", selection: sortBinding) {
                        ForEach(LibrarySort.allCases) { order in
                            Text(order.label).tag(order)
                        }
                    }
                    .accessibilityIdentifier("settings.sort")
                    Toggle("Online catalogue search", isOn: catalogueBinding)
                    .accessibilityLabel("Online catalogue search")
                    .accessibilityIdentifier("settings.onlineCatalogue")
                } header: {
                    Text("Preferences")
                } footer: {
                    Text("When online search is off, Pocket Library never connects to the internet except to show covers of books you added from the catalogue before.")
                }

                VoiceSettingsSection()

                Section {
                    if let storage {
                        LabeledContent("Books", value: "\(storage.books)")
                        LabeledContent("History entries", value: "\(storage.activityEntries)")
                        LabeledContent("Saved covers", value: "\(storage.savedCovers)")
                        LabeledContent("Saved EPUBs", value: "\(storage.savedBooks)")
                        LabeledContent("Space used", value: Int64(storage.bytes).formatted(.byteCount(style: .file)))
                    }
                    if let exportURL {
                        ShareLink(item: exportURL) {
                            Text("Share Export File")
                        }
                        .accessibilityIdentifier("settings.shareExport")
                    } else {
                        Button {
                            export()
                        } label: {
                            LabeledContent("Export Library") {
                                if exporting { ProgressView() }
                            }
                        }
                        .disabled(exporting)
                        .accessibilityIdentifier("settings.export")
                    }
                    Button("Erase Library…", role: .destructive) {
                        alert = .confirm(
                            "Erase your whole library?",
                            message: "All books, notes, history and saved covers are deleted from this iPhone. This can't be undone.",
                            confirm: "Erase Library",
                            action: .erase,
                            identifier: "settings.erase",
                            destructive: true
                        )
                    }
                        .accessibilityIdentifier("settings.erase")
                } header: {
                    Text("Your Data")
                } footer: {
                    Text("Your library is stored only on this iPhone. Export it to keep a copy elsewhere; the file includes your books, notes and history.")
                }

                Section("About") {
                    LabeledContent("Book data", value: "Open Library")
                    LabeledContent("Built with", value: "Forge (Rust + SwiftUI)")
                }
            }
            .navigationTitle("Settings")
            .onAppear { storage = model.storageInfo() }
            .onChange(of: model.totalBooks) { _, _ in
                storage = model.storageInfo()
                exportURL = nil
            }
            .forgeAlert($alert) { action in
                switch action {
                case .erase: erase()
                }
            }
        }
    }

    private var sortBinding: Binding<LibrarySort> {
        Binding(
            get: { model.settings.defaultSort },
            set: { sort in
                var settings = model.settings
                settings.defaultSort = sort
                save(settings)
                // The library list follows the new default straight away.
                model.sort = sort
            }
        )
    }

    private var catalogueBinding: Binding<Bool> {
        Binding(
            get: { model.settings.catalogueEnabled },
            set: { enabled in
                var settings = model.settings
                settings.catalogueEnabled = enabled
                save(settings)
            }
        )
    }

    /// Saves through Rust; a failure is shown and the control snaps back,
    /// because it always shows what Rust has.
    private func save(_ settings: LibrarySettings) {
        Task {
            do {
                try await model.updateSettings(settings)
            } catch {
                alert = .failure(error)
            }
        }
    }

    private func export() {
        exporting = true
        Task {
            defer { exporting = false }
            do {
                exportURL = try await model.exportLibrary()
            } catch {
                alert = .failure(error)
            }
        }
    }

    private func erase() {
        Task {
            do {
                try await model.eraseLibrary()
                storage = model.storageInfo()
                exportURL = nil
            } catch {
                alert = .failure(error)
            }
        }
    }
}
