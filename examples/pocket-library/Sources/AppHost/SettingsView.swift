@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// The Settings tab: preferences, and the user's local data.
struct SettingsView: View {
    enum SettingsAction {
        case erase
        case restore(URL)
    }

    /// What the document picker is choosing.
    enum Picking { case spreadsheet, backup }

    @Environment(LibraryModel.self) private var model
    @State private var exportURL: URL?
    @State private var exporting = false
    @State private var csvURL: URL?
    @State private var picking: Picking?
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
                            Text("Share Backup")
                        }
                        .accessibilityIdentifier("settings.shareExport")
                    } else {
                        Button {
                            export()
                        } label: {
                            LabeledContent("Back Up Library") {
                                if exporting { ProgressView() }
                            }
                        }
                        .disabled(exporting)
                        .accessibilityIdentifier("settings.export")
                    }
                    Button("Restore from Backup…") { picking = .backup }
                        .accessibilityIdentifier("settings.restore")
                    if let csvURL {
                        ShareLink(item: csvURL) {
                            Text("Share Spreadsheet")
                        }
                        .accessibilityIdentifier("settings.shareCSV")
                    } else {
                        Button("Export as Spreadsheet") { exportCSV() }
                            .disabled(exporting)
                            .accessibilityIdentifier("settings.exportCSV")
                    }
                    Button("Import from Spreadsheet…") { picking = .spreadsheet }
                        .accessibilityIdentifier("settings.importCSV")
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
                    Text("Your library is stored only on this iPhone. Back it up to keep a copy elsewhere: the backup has your books, notes, history, covers and EPUBs, and restores on this iPhone or another. A spreadsheet holds your books only, and opens in Numbers or Excel.")
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
                csvURL = nil
            }
            // One picker for both kinds: SwiftUI shows only one
            // .fileImporter per view.
            .fileImporter(
                isPresented: Binding(get: { picking != nil }, set: { if !$0 { picking = nil } }),
                allowedContentTypes: picking == .backup ? [.zip] : [.commaSeparatedText]
            ) { result in
                let kind = picking
                switch result {
                case .success(let url) where kind == .backup:
                    alert = .confirm(
                        "Replace your library with this backup?",
                        message: "Everything in the library now is replaced by the backup's books, notes, history, covers and EPUBs.",
                        confirm: "Restore",
                        action: .restore(url),
                        identifier: "settings.restoreConfirm",
                        destructive: true
                    )
                case .success(let url): importCSV(url)
                case .failure(let error): alert = .failure(error)
                }
            }
            .forgeAlert($alert) { action in
                switch action {
                case .erase: erase()
                case .restore(let url): restore(url)
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
                exportURL = try await model.backupLibrary()
            } catch {
                alert = .failure(error)
            }
        }
    }

    private func exportCSV() {
        exporting = true
        Task {
            defer { exporting = false }
            do {
                csvURL = try await model.exportBooksCSV()
            } catch {
                alert = .failure(error)
            }
        }
    }

    /// Imports the chosen spreadsheet, then says how many books came in and
    /// which rows were skipped.
    private func importCSV(_ url: URL) {
        Task {
            do {
                let summary = try await model.importBooksCSV(at: url)
                let added = summary.added == 1 ? "1 book added" : "\(summary.added) books added"
                var message: String?
                if !summary.skipped.isEmpty {
                    let shown = summary.skipped.prefix(5).joined(separator: "\n")
                    let more = summary.skipped.count > 5 ? "\nand \(summary.skipped.count - 5) more" : ""
                    message = "\(summary.skipped.count) rows skipped:\n\(shown)\(more)"
                }
                alert = ForgeAlert(title: added, message: message, buttons: [.ok])
                storage = model.storageInfo()
            } catch {
                alert = .failure(error)
            }
        }
    }

    private func restore(_ url: URL) {
        Task {
            do {
                let books = try await model.restoreLibrary(at: url)
                storage = model.storageInfo()
                exportURL = nil
                csvURL = nil
                alert = ForgeAlert(
                    title: "Library restored",
                    message: books == 1 ? "1 book" : "\(books) books",
                    buttons: [.ok])
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
                csvURL = nil
            } catch {
                alert = .failure(error)
            }
        }
    }
}
