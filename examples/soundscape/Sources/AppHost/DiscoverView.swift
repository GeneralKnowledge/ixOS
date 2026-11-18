#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// MARK: Discover

/// Free music to find and play: Jamendo search (this week's popular until
/// you type), each track with its licence. Plays through the same player;
/// Add to Library keeps a track with your music, where it can be put in
/// playlists and, where its licence allows, downloaded.
struct DiscoverView: View {
    let model: ListenModel
    let player: TakePlayer

    @State private var search = ""
    @State private var results: [CatalogueTrack] = []
    @State private var problem: String?
    @State private var searching = false
    @State private var editingClientId = false
    @State private var attempt = 0

    private var query: String { search.trimmingCharacters(in: .whitespaces) }

    var body: some View {
        Group {
            if model.jamendoClientId == nil {
                JamendoClientIdForm(model: model, done: nil)
            } else {
                list
            }
        }
        .miniPlayerClearance()
        .navigationTitle("Discover")
        .toolbar {
            if model.jamendoClientId != nil {
                ToolbarItem(placement: .secondaryAction) {
                    Button("Jamendo Client ID", systemImage: "key") { editingClientId = true }
                        .accessibilityIdentifier("discover.clientId")
                }
            }
        }
        .sheet(isPresented: $editingClientId) {
            NavigationStack {
                JamendoClientIdForm(model: model) { editingClientId = false }
                    .navigationTitle("Jamendo")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Cancel") { editingClientId = false }
                                .accessibilityIdentifier("discover.clientIdCancel")
                        }
                    }
            }
            .presentationDetents([.medium, .large])
        }
        .forgeFeedback(model.feedback)
    }

    private var list: some View {
        List {
            Section {
                PlayShuffleButtons(disabled: results.isEmpty) { shuffle in
                    player.play(model.queue(results), shuffle: shuffle)
                }
            }
            Section {
                ForEach(Array(results.enumerated()), id: \.element.id) { index, track in
                    CatalogueRow(model: model, player: player, track: track) {
                        player.play(model.queue(results), from: index)
                    }
                }
            } header: {
                if !results.isEmpty {
                    Text(query.isEmpty ? "Popular This Week" : "Results")
                }
            } footer: {
                if !results.isEmpty {
                    Text("Music from Jamendo, each track under its artist's Creative Commons licence.")
                }
            }
        }
        .searchable(text: $search, prompt: "Search Jamendo")
        .refreshable { attempt += 1 }
        .task(id: "\(model.jamendoClientId ?? "")|\(query)|\(attempt)") { await find() }
        .overlay {
            if let problem, results.isEmpty {
                ForgeStateView.failed(
                    "Can't Search Jamendo", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "discover.retry") { attempt += 1 }
                )
            } else if searching && results.isEmpty {
                ForgeStateView.loading("Searching Jamendo")
            } else if !searching && results.isEmpty && !query.isEmpty {
                ForgeStateView.noResults(for: query)
            }
        }
    }

    /// Searches after a short pause in typing; a newer search cancels this.
    private func find() async {
        if !query.isEmpty {
            try? await Task.sleep(for: .milliseconds(400))
            if Task.isCancelled { return }
        }
        searching = true
        defer { searching = false }
        switch await model.searchJamendo(query) {
        case .success(let found):
            results = found
            problem = nil
        case .failure(let failure):
            if failure.stopped { return }
            problem = failure.message
        }
    }
}

/// One catalogue track: plays on tap, with its licence, and adds to the
/// library with the plus.
struct CatalogueRow: View {
    let model: ListenModel
    let player: TakePlayer
    let track: CatalogueTrack
    let play: () -> Void

    private var isCurrent: Bool { player.current?.id == track.id && !player.isComparing }
    private var subtitle: String {
        [track.artist ?? "Unknown Artist", track.album].compactMap { $0 }.joined(separator: " · ")
    }
    private var facts: String {
        [track.durationSeconds.map(TakeFormat.time), track.rights.licence].compactMap { $0 }
            .joined(separator: " · ")
    }

    var body: some View {
        let added = model.inLibrary(track.id)
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    Artwork(title: track.title, url: track.artworkUrl, size: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            Text(track.title)
                                .foregroundStyle(isCurrent ? Color.accentColor : .primary)
                                .lineLimit(1)
                            if isCurrent && player.isPlaying {
                                Image(systemName: "speaker.wave.2.fill")
                                    .font(.caption)
                                    .foregroundStyle(.tint)
                                    .accessibilityLabel("Playing")
                            }
                        }
                        Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        Text(facts).font(.caption2).foregroundStyle(.tertiary).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(track.title), \(subtitle), \(facts)")
            .accessibilityHint("Plays it")
            .accessibilityIdentifier("discover.track")
            if added {
                Image(systemName: "checkmark.circle.fill")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .frame(width: 32, height: 32)
                    .accessibilityLabel("In your library")
                    .accessibilityIdentifier("discover.inLibrary")
            } else {
                Button {
                    Task { await model.addToLibrary([track]) }
                } label: {
                    Image(systemName: "plus.circle")
                        .font(.title3)
                        .frame(width: 32, height: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Add \(track.title) to Library")
                .accessibilityIdentifier("discover.add")
            }
        }
        .contextMenu {
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: true)
            }
            .accessibilityIdentifier("discover.playNext")
            Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                player.add(model.queue([track]), next: false)
            }
            .accessibilityIdentifier("discover.addToQueue")
            if !added {
                Button("Add to Library", systemImage: "plus") {
                    Task { await model.addToLibrary([track]) }
                }
                .accessibilityIdentifier("discover.addToLibrary")
            }
            if let page = track.rights.pageUrl.flatMap(URL.init(string:)) {
                Link(destination: page) {
                    Label("Open on Jamendo", systemImage: "safari")
                }
                .accessibilityIdentifier("discover.open")
            }
        }
    }
}

// MARK: Jamendo client ID

/// The Jamendo client ID field, with where to get one. Shown on its own
/// before there is one, and in a sheet to change it.
struct JamendoClientIdForm: View {
    let model: ListenModel
    let done: (() -> Void)?

    @State private var clientId = ""
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        Form {
            Section {
                TextField("Client ID", text: $clientId)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.done)
                    .onSubmit(save)
                    .accessibilityLabel("Jamendo client ID")
                    .accessibilityIdentifier("discover.clientIdField")
                Button(saving ? "Saving…" : "Save", action: save)
                    .disabled(saving || clientId.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("discover.clientIdSave")
            } header: {
                Text("Jamendo client ID")
            } footer: {
                Text("Soundscape searches Jamendo with your own free client ID, for non-commercial use.")
            }
            if let problem {
                Section { Text(problem).forgeForeground(.danger) }
            }
            Section("Getting one") {
                Text("Sign in at Jamendo's developer site, create an app, and paste its Client ID here.")
                if let site = URL(string: "https://devportal.jamendo.com") {
                    Link("Open devportal.jamendo.com", destination: site)
                        .accessibilityIdentifier("discover.devPortal")
                }
            }
            if model.jamendoClientId != nil {
                Section {
                    Button("Forget Client ID", role: .destructive) {
                        Task {
                            do {
                                try await model.setJamendoClientId("")
                                done?()
                            } catch {
                                problem = ListenModel.message(error)
                            }
                        }
                    }
                    .accessibilityIdentifier("discover.clientIdForget")
                } footer: {
                    Text("Jamendo songs already in your library stay.")
                }
            }
        }
        .onAppear { if clientId.isEmpty { clientId = model.jamendoClientId ?? "" } }
    }

    private func save() {
        saving = true
        problem = nil
        let typed = clientId
        Task {
            defer { saving = false }
            do {
                try await model.setJamendoClientId(typed)
                done?()
            } catch {
                problem = ListenModel.message(error)
            }
        }
    }
}
#endif
