#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// Smart playlists (M10, docs/roadmap.md): playlists made from rules, worked
// out by Rust from the library each time they show.

/// The smart playlists at the top of Playlists: the built-in ones, then yours.
struct SmartPlaylistsSection: View {
    let model: ListenModel

    var body: some View {
        Section("Smart Playlists") {
            ForEach(model.collection.smartPlaylists, id: \.id) { list in
                NavigationLink(value: ListenRoute.smartPlaylist(list.id)) { SmartPlaylistRow(list: list) }
                    .accessibilityIdentifier(list.builtIn ? "playlists.builtInSmart" : "playlists.smart")
                    .forgeRowActions {
                        if !list.builtIn {
                            Button("Delete", systemImage: "trash", role: .destructive) {
                                Task { await model.deleteSmartPlaylist(list.id) }
                            }
                            .accessibilityIdentifier("playlists.deleteSmart")
                        }
                    }
            }
        }
    }
}

struct SmartPlaylistRow: View {
    let list: SmartPlaylist

    var body: some View {
        HStack(spacing: 12) {
            SmartPlaylistArtwork(list: list, size: 56)
            VStack(alignment: .leading, spacing: 2) {
                Text(list.name).font(.headline).lineLimit(1)
                Text(TakeFormat.smartSummary(list))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// Four of its songs' pictures, or one, or a gear on the stand-in.
struct SmartPlaylistArtwork: View {
    let list: SmartPlaylist
    let size: CGFloat

    var body: some View {
        let urls = list.artworkUrls
        Group {
            if urls.count >= 4 {
                let half = size / 2
                VStack(spacing: 0) {
                    HStack(spacing: 0) {
                        Artwork(title: list.name, url: urls[0], size: half)
                        Artwork(title: list.name, url: urls[1], size: half)
                    }
                    HStack(spacing: 0) {
                        Artwork(title: list.name, url: urls[2], size: half)
                        Artwork(title: list.name, url: urls[3], size: half)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: size * 0.08))
            } else if let url = urls.first {
                Artwork(title: list.name, url: url, size: size)
            } else {
                RoundedRectangle(cornerRadius: size * 0.08)
                    .fill(.tint.opacity(0.15))
                    .frame(width: size, height: size)
                    .overlay {
                        Image(systemName: "gearshape.fill")
                            .font(.system(size: size * 0.4))
                            .foregroundStyle(.tint)
                    }
            }
        }
        .accessibilityHidden(true)
    }
}

/// One smart playlist: its rules in words, Play and Shuffle, its songs.
struct SmartPlaylistView: View {
    let model: ListenModel
    let player: TakePlayer
    let id: String

    @State private var detail: SmartPlaylistDetail?
    @State private var editing: SmartPlaylistDraft?
    @State private var freezing = false
    @State private var details: String?
    @State private var confirmingDelete = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        Group {
            if let detail {
                content(detail)
            } else {
                ForgeStateView.empty(
                    "Smart Playlist Deleted", systemImage: "gearshape",
                    description: "It isn't in your playlists any more."
                )
            }
        }
        .navigationTitle(detail?.playlist.name ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { detail = model.smartPlaylist(id) }
        // Plays, favourites and edits change what the rules let in.
        .onChange(of: model.collection) { detail = model.smartPlaylist(id) }
        .onChange(of: model.library) { detail = model.smartPlaylist(id) }
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            if let detail {
                ToolbarItem(placement: .primaryAction) { menu(detail) }
            }
        }
        .sheet(item: Binding(
            get: { editing.map { EditorItem(draft: $0) } },
            set: { if $0 == nil { editing = nil } }
        )) { item in
            SmartPlaylistEditor(model: model, editing: detail?.playlist.builtIn == false ? id : nil,
                                start: item.draft) { saved in
                if saved.playlist.id == id {
                    detail = saved
                }
            }
        }
        .forgeRename("Make Playlist", isPresented: $freezing, name: detail?.playlist.name ?? "",
                     id: "playlistName", confirmTitle: "Make") { name in
            _ = try await model.createPlaylist(name, with: detail?.entries.map(\.track.id) ?? [])
        }
        .confirmationDialog("Delete \(detail?.playlist.name ?? "this smart playlist")?",
                            isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button("Delete Smart Playlist", role: .destructive) {
                Task {
                    await model.deleteSmartPlaylist(id)
                    dismiss()
                }
            }
            .accessibilityIdentifier("smartPlaylist.confirmDelete")
        } message: {
            Text("The songs stay in your library.")
        }
        .forgeFeedback(model.feedback)
    }

    private func menu(_ detail: SmartPlaylistDetail) -> some View {
        Menu("More", systemImage: "ellipsis.circle") {
            let list = detail.playlist
            if list.builtIn {
                Button("Make Your Own from These Rules", systemImage: "plus.square.on.square") {
                    editing = SmartPlaylistDraft(name: "\(list.name) (Mine)", rules: list.rules)
                }
                .accessibilityIdentifier("smartPlaylist.copy")
            } else {
                Button("Edit Rules", systemImage: "slider.horizontal.3") {
                    editing = SmartPlaylistDraft(name: list.name, rules: list.rules)
                }
                .accessibilityIdentifier("smartPlaylist.editRules")
            }
            Button("Make Playlist of These Songs", systemImage: "music.note.list") { freezing = true }
                .disabled(detail.entries.isEmpty)
                .accessibilityIdentifier("smartPlaylist.makePlaylist")
            DownloadMenuItem(model: model, tracks: detail.entries.map(\.track),
                             identifier: "smartPlaylist.download")
            if !list.builtIn {
                Divider()
                Button("Delete Smart Playlist", systemImage: "trash", role: .destructive) {
                    confirmingDelete = true
                }
                .accessibilityIdentifier("smartPlaylist.delete")
            }
        }
        .accessibilityIdentifier("smartPlaylist.more")
    }

    private func content(_ detail: SmartPlaylistDetail) -> some View {
        let tracks = detail.entries.map(\.track)
        let counted = detail.playlist.rules.sort == .mostPlayed
        return List {
            Section {
                VStack(spacing: 10) {
                    SmartPlaylistArtwork(list: detail.playlist, size: 200)
                        .shadow(radius: 10, y: 5)
                    Text(detail.playlist.name).font(.title2.bold()).multilineTextAlignment(.center)
                    Text(TakeFormat.smartSummary(detail.playlist))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(Self.rulesText(detail.playlist.rules))
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .accessibilityIdentifier("smartPlaylist.rules")
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            Section {
                PlayShuffleButtons(disabled: tracks.isEmpty) { shuffle in
                    player.play(model.queue(tracks), shuffle: shuffle)
                }
            }
            Section {
                ForEach(Array(detail.entries.enumerated()), id: \.element.track.id) { index, entry in
                    TrackRow(model: model, player: player, track: entry.track,
                             showDetails: { details = entry.track.id }) {
                        player.play(model.queue(tracks), from: index)
                    }
                    .badge(counted ? Text("\(entry.playCount)") : nil)
                }
            }
        }
        .overlay {
            if detail.entries.isEmpty {
                ForgeStateView.empty(
                    "No Songs Match", systemImage: "gearshape",
                    description: "Songs show here as soon as they match its rules."
                )
            }
        }
    }

    /// "Songs matching all of: Artist contains “Hours”, Plays at least 5.
    /// Most Played first, at most 25."
    static func rulesText(_ rules: SmartRules) -> String {
        var text: String
        if rules.rules.isEmpty {
            text = "Every song"
        } else {
            let each = rules.rules.map(TakeFormat.smartRule).joined(separator: "; ")
            text = rules.rules.count == 1 ? each : "\(rules.matchAll ? "All of" : "Any of"): \(each)"
        }
        text += ". By \(TakeFormat.smartSort(rules.sort).lowercased())"
        if let limit = rules.limit { text += ", at most \(limit)" }
        return text + "."
    }
}

/// A draft in a sheet: a new identity each time the editor opens.
private struct EditorItem: Identifiable {
    let id = UUID()
    let draft: SmartPlaylistDraft
}

// MARK: The editor

/// Name, rules, order and limit, with how many songs match as you go.
/// `editing` is your smart playlist's id; nil makes a new one.
struct SmartPlaylistEditor: View {
    let model: ListenModel
    let editing: String?
    let start: SmartPlaylistDraft
    var saved: (SmartPlaylistDetail) -> Void = { _ in }

    @State private var draft: SmartPlaylistDraft
    @State private var saving = false
    @State private var problem: String?
    @Environment(\.dismiss) private var dismiss

    init(model: ListenModel, editing: String?, start: SmartPlaylistDraft,
         saved: @escaping (SmartPlaylistDetail) -> Void = { _ in }) {
        self.model = model
        self.editing = editing
        self.start = start
        self.saved = saved
        _draft = State(initialValue: start)
    }

    private var matching: UInt32? { model.smartPlaylistCount(draft.smartRules) }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $draft.name)
                        .accessibilityIdentifier("smartEditor.name")
                }
                Section {
                    if draft.rules.count > 1 {
                        Picker("Songs must match", selection: $draft.matchAll) {
                            Text("All Rules").tag(true)
                            Text("Any Rule").tag(false)
                        }
                        .pickerStyle(.segmented)
                        .accessibilityIdentifier("smartEditor.match")
                    }
                    ForEach($draft.rules) { $rule in
                        SmartRuleEditor(rule: $rule)
                    }
                    .onDelete { draft.rules.remove(atOffsets: $0) }
                    Menu {
                        ForEach(SmartRuleDraft.Kind.allCases) { kind in
                            Button(kind.label) { draft.rules.append(SmartRuleDraft(kind: kind)) }
                        }
                    } label: {
                        Label("Add Rule", systemImage: "plus.circle")
                    }
                    .disabled(draft.rules.count >= 20)
                    .accessibilityIdentifier("smartEditor.addRule")
                } header: {
                    Text("Rules")
                } footer: {
                    Text(matchingText).accessibilityIdentifier("smartEditor.matching")
                }
                Section("Order") {
                    Picker("Sort by", selection: $draft.sort) {
                        ForEach(Self.sorts, id: \.self) { sort in
                            Text(TakeFormat.smartSort(sort)).tag(sort)
                        }
                    }
                    .accessibilityIdentifier("smartEditor.sort")
                    Toggle("Limit the Number of Songs", isOn: $draft.limited)
                        .accessibilityIdentifier("smartEditor.limited")
                    if draft.limited {
                        Stepper("At most \(draft.limit) songs", value: $draft.limit, in: 1...5000, step: 5)
                            .accessibilityIdentifier("smartEditor.limit")
                    }
                }
                if let problem {
                    Section {
                        Text(problem).forgeForeground(.danger)
                            .accessibilityIdentifier("smartEditor.problem")
                    }
                }
            }
            .navigationTitle(editing == nil ? "New Smart Playlist" : "Edit Smart Playlist")
            .navigationBarTitleDisplayMode(.inline)
            .forgeFormToolbar(
                id: "smartEditor",
                confirmTitle: editing == nil ? "Create" : "Save",
                canConfirm: !draft.name.trimmingCharacters(in: .whitespaces).isEmpty && matching != nil,
                isWorking: saving,
                hasChanges: draft != start,
                cancel: { dismiss() },
                confirm: save
            )
        }
    }

    private var matchingText: String {
        guard let matching else { return "Finish the rules to see which songs match." }
        return matching == 1 ? "1 song matches now." : "\(matching) songs match now."
    }

    private func save() {
        saving = true
        problem = nil
        Task {
            defer { saving = false }
            do {
                let detail: SmartPlaylistDetail
                if let editing {
                    detail = try await model.editSmartPlaylist(editing, name: draft.name, rules: draft.smartRules)
                } else {
                    detail = try await model.createSmartPlaylist(draft.name, rules: draft.smartRules)
                }
                saved(detail)
                dismiss()
            } catch {
                problem = error.localizedDescription
            }
        }
    }

    static let sorts: [SmartSort] = [
        .title, .artist, .album, .recentlyAdded, .mostPlayed, .recentlyPlayed, .leastRecentlyPlayed,
    ]
}

/// One rule: what it looks at, how it compares, and with what.
struct SmartRuleEditor: View {
    @Binding var rule: SmartRuleDraft

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Rule", selection: $rule.kind) {
                ForEach(SmartRuleDraft.Kind.allCases) { kind in
                    Text(kind.label).tag(kind)
                }
            }
            .accessibilityIdentifier("smartEditor.ruleKind")
            switch rule.kind {
            case .title, .artist, .album, .genre:
                Picker("Test", selection: $rule.textTest) {
                    ForEach(Self.textTests, id: \.self) { test in
                        Text(TakeFormat.smartTest(test)).tag(test)
                    }
                }
                .accessibilityIdentifier("smartEditor.textTest")
                TextField(rule.kind.label, text: $rule.text)
                    .textInputAutocapitalization(.never)
                    .accessibilityIdentifier("smartEditor.text")
            case .plays, .length:
                Picker("Test", selection: $rule.numberTest) {
                    Text("At least").tag(SmartNumberTest.atLeast)
                    Text("At most").tag(SmartNumberTest.atMost)
                    Text("Exactly").tag(SmartNumberTest.equals)
                }
                .pickerStyle(.segmented)
                .accessibilityIdentifier("smartEditor.numberTest")
                let unit = rule.kind == .plays
                    ? (Int(rule.number) == 1 ? "play" : "plays")
                    : (Int(rule.number) == 1 ? "minute" : "minutes")
                Stepper("\(Int(rule.number)) \(unit)", value: $rule.number, in: 0...10_000, step: 1)
                    .accessibilityIdentifier("smartEditor.number")
            case .added, .lastPlayed:
                Picker("Test", selection: $rule.dateTest) {
                    Text("In the last").tag(SmartDateTest.inLast)
                    Text("Not in the last").tag(SmartDateTest.notInLast)
                }
                .pickerStyle(.segmented)
                .accessibilityIdentifier("smartEditor.dateTest")
                Stepper("\(rule.days) \(rule.days == 1 ? "day" : "days")", value: $rule.days, in: 1...36_500)
                    .accessibilityIdentifier("smartEditor.days")
            case .favourite:
                Picker("Favourite", selection: $rule.favourite) {
                    Text("Is a favourite").tag(true)
                    Text("Isn’t a favourite").tag(false)
                }
                .pickerStyle(.segmented)
                .accessibilityIdentifier("smartEditor.favourite")
            }
        }
        .padding(.vertical, 4)
    }

    static let textTests: [SmartTextTest] = [.contains, .doesNotContain, .equals, .doesNotEqual, .beginsWith]
}
#endif
