@preconcurrency import BusinessPreviewFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the prospects once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<BusinessPreviewCore>

    public init(host: ForgeHost<BusinessPreviewCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            ProspectsScreen(core: core, changes: host.changes)
        }
        .forgeTheme(ForgeTheme(accent: .indigo))
    }
}

/// The businesses imported on this iPhone. Tapping one opens its preview
/// full screen, over Device Lab's own controls.
private struct ProspectsScreen: View {
    @State private var model: ProspectsModel
    @State private var picking = false
    @State private var typingAddress = false
    /// The business whose preview is open.
    @State private var shown: Prospect?
    let changes: UInt64

    init(core: BusinessPreviewCore, changes: UInt64) {
        _model = State(initialValue: ProspectsModel(core: core))
        self.changes = changes
    }

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Prospects")
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Menu {
                            Button("From Files", systemImage: "folder") { picking = true }
                                .accessibilityIdentifier("prospects.import.files")
                            Button("From a Web Address", systemImage: "link") { typingAddress = true }
                                .accessibilityIdentifier("prospects.import.address")
                        } label: {
                            Label("Import a Pack", systemImage: "plus")
                        }
                        .accessibilityIdentifier("prospects.import")
                    }
                }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.zip, .json]) { result in
            switch result {
            case .success(let url): model.importPicked(url)
            case .failure(let error): model.alert = .failure(error)
            }
        }
        .sheet(isPresented: $typingAddress) {
            AddressSheet(model: model)
        }
        .fullScreenCover(item: $shown) { prospect in
            PreviewRoot(core: model.core, prospect: prospect)
        }
        // Held back while the address sheet or a preview is up: each shows its own.
        .forgeAlert(Binding(
            get: { typingAddress || shown != nil ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in model.handle(action) }
        .forgeFeedback(model.feedback)
        .onChange(of: changes) { model.reload() }
    }

    @ViewBuilder
    private var content: some View {
        if model.prospects.isEmpty && !model.importing {
            ForgeStateView.empty(
                "No Prospects Yet",
                systemImage: "storefront",
                description: "Import a business's profile pack, made from its website by the extractor on your computer.",
                action: ForgeStateAction("Import from Files", identifier: "prospects.empty.import") { picking = true }
            )
        } else {
            List {
                if model.importing {
                    HStack {
                        ProgressView()
                        Text("Reading the pack…")
                            .foregroundStyle(.secondary)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("prospects.importing")
                }
                ForEach(model.prospects) { prospect in
                    row(prospect)
                }
            }
        }
    }

    private func row(_ prospect: Prospect) -> some View {
        let deleting = model.deleting.contains(prospect.id)
        return Button {
            shown = prospect
        } label: {
            ForgeRow(prospect.name, subtitle: Self.subtitle(prospect), systemImage: "storefront") {
                if deleting {
                    ProgressView()
                } else {
                    ForgeBadge(prospect.isConcept ? "Concept" : "Approved", tone: prospect.isConcept ? .neutral : .success)
                }
            }
        }
        .disabled(deleting)
        .accessibilityIdentifier("prospects.row.\(prospect.id)")
        .accessibilityHint("Opens the business's preview.")
        .forgeRowActions {
            Button("Delete", systemImage: "trash", role: .destructive) { model.askToDelete(prospect) }
                .accessibilityIdentifier("prospects.delete")
        }
    }

    private static func subtitle(_ prospect: Prospect) -> String {
        switch prospect.unverified {
        case 0: prospect.category
        case 1: "\(prospect.category), 1 value to check"
        default: "\(prospect.category), \(prospect.unverified) values to check"
        }
    }
}

/// Downloads a pack from an https address (the laptop's file share, or the
/// project's server).
private struct AddressSheet: View {
    let model: ProspectsModel
    @State private var address = ""
    @State private var working = false
    @State private var failure: ForgeFailure?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("https://", text: $address)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .textContentType(.URL)
                        .accessibilityLabel("Pack address")
                        .accessibilityIdentifier("address.field")
                } footer: {
                    Text("The address of a pack's zip or profile.json. Packs are downloaded over https only.")
                }
            }
            .navigationTitle("Import from Address")
            .navigationBarTitleDisplayMode(.inline)
            .forgeFormToolbar(
                id: "address",
                confirmTitle: "Import",
                canConfirm: !address.trimmingCharacters(in: .whitespaces).isEmpty,
                isWorking: working,
                hasChanges: !address.isEmpty,
                cancel: { dismiss() },
                confirm: importPack
            )
            .forgeAlert(failure: $failure)
        }
    }

    private func importPack() {
        working = true
        Task {
            failure = await model.download(address: address)
            working = false
            if failure == nil { dismiss() }
        }
    }
}
