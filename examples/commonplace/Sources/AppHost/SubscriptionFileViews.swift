@preconcurrency import CommonplaceFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Import Subscriptions: what an OPML file holds, before anything changes.
/// New feeds start picked; feeds already subscribed are left exactly as
/// they are; a feed listed twice or with a bad address can't be picked.
/// Settings Commonplace doesn't use are named, so nothing is dropped
/// silently.
struct ImportSubscriptionsView: View {
    let model: LibraryModel
    let draft: OpmlDraft
    /// Called with what the import did; the sheet closes.
    let imported: (OpmlImport) -> Void

    @State private var picked: Set<UInt32>
    @State private var importing = false
    @Environment(\.dismiss) private var dismiss

    init(model: LibraryModel, draft: OpmlDraft, imported: @escaping (OpmlImport) -> Void) {
        self.model = model
        self.draft = draft
        self.imported = imported
        _picked = State(initialValue: draft.picked)
    }

    private var preview: OpmlPreview { draft.preview }
    private var pickable: [OpmlFeed] { preview.feeds.filter { $0.status == .new } }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text(summary)
                        .accessibilityIdentifier("import.summary")
                    if !pickable.isEmpty {
                        Button(picked.count == pickable.count ? "Pick None" : "Pick All") {
                            picked = picked.count == pickable.count ? [] : Set(pickable.map(\.index))
                        }
                        .accessibilityIdentifier("import.pickAll")
                    }
                }
                ForEach(groups) { group in
                    Section(group.name) {
                        ForEach(group.feeds) { feed in
                            row(feed)
                        }
                    }
                }
                if !notCarried.isEmpty {
                    Section {
                        ForEach(notCarried, id: \.self) { Text($0) }
                    } header: {
                        Text("Not Carried Over")
                    } footer: {
                        Text("Commonplace doesn't use these settings. A feed's own ones are kept and written back when you export.")
                    }
                    .accessibilityIdentifier("import.notCarried")
                }
            }
            .navigationTitle("Import Subscriptions")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "import", confirmTitle: "Import", canConfirm: !(picked.isEmpty && preview.filters == 0),
                isWorking: importing, cancel: { dismiss() }
            ) { Task { await runImport() } }
        }
    }

    private func row(_ feed: OpmlFeed) -> some View {
        let canPick = feed.status == .new
        return Toggle(isOn: Binding(
            get: { picked.contains(feed.index) },
            set: { on in
                if on { picked.insert(feed.index) } else { picked.remove(feed.index) }
            }
        )) {
            VStack(alignment: .leading, spacing: 2) {
                Text(feed.title)
                Text(statusText(feed))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .disabled(!canPick)
        .accessibilityIdentifier("import.feed.\(feed.index)")
    }

    private func statusText(_ feed: OpmlFeed) -> String {
        switch feed.status {
        case .new: return feed.url
        case .alreadySubscribed: return String(localized: "Already subscribed: left as it is")
        case .listedTwice: return String(localized: "Listed twice in this file")
        case .badAddress: return String(localized: "Not a web address Commonplace can fetch")
        }
    }

    /// Feeds by the folder path they sit in, in the file's order.
    private var groups: [FeedGroup] {
        var order: [String] = []
        var byName: [String: [OpmlFeed]] = [:]
        for feed in preview.feeds {
            let name = feed.folder.isEmpty ? String(localized: "No Folder") : feed.folder.joined(separator: " / ")
            if byName[name] == nil { order.append(name) }
            byName[name, default: []].append(feed)
        }
        return order.map { FeedGroup(name: $0, feeds: byName[$0] ?? []) }
    }

    private var notCarried: [String] {
        var names = Set(preview.settingsNotUsed)
        for feed in preview.feeds { names.formUnion(feed.notUsed) }
        return names.sorted()
    }

    private var summary: String {
        var parts = [
            inflected("^[\(pickable.count) new feed](inflect: true) to add"),
        ]
        let existing = preview.feeds.filter { if case .alreadySubscribed = $0.status { return true } else { return false } }.count
        if existing > 0 {
            parts.append(inflected("^[\(existing) feed](inflect: true) already subscribed"))
        }
        if !preview.folders.isEmpty {
            parts.append(inflected("^[\(preview.folders.count) folder](inflect: true)"))
        }
        if preview.filters > 0 {
            parts.append(inflected("^[\(Int(preview.filters)) filter](inflect: true), kept as text"))
        }
        if preview.rejectedFilters > 0 {
            parts.append(inflected("^[\(Int(preview.rejectedFilters)) filter](inflect: true) refused"))
        }
        return parts.joined(separator: ". ") + "."
    }

    private func runImport() async {
        importing = true
        defer { importing = false }
        if let done = await model.importSubscriptions(draft, picked: picked) {
            dismiss()
            imported(done)
        }
    }
}

/// The feeds of one folder path in an import preview.
private struct FeedGroup: Identifiable {
    let name: String
    let feeds: [OpmlFeed]
    var id: String { name }
}

