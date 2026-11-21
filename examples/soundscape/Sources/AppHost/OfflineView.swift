#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

// MARK: Downloading

/// Download or Remove Download for some songs (one, an album, a playlist),
/// for a menu.
struct DownloadMenuItem: View {
    let model: ListenModel
    let tracks: [LibraryTrack]
    let identifier: String

    var body: some View {
        if !model.downloadable(tracks).isEmpty {
            let chosen = model.allChosenForPhone(tracks)
            Button(chosen ? "Remove Download" : "Download",
                   systemImage: chosen ? "minus.circle" : "arrow.down.circle") {
                Task { await model.keepOnPhone(tracks, !chosen) }
            }
            .accessibilityIdentifier(identifier)
        }
    }
}

/// Download Album on an album's screen, with how far along it is, or
/// Remove Download once it is all chosen.
struct AlbumDownloadButton: View {
    let model: ListenModel
    let tracks: [LibraryTrack]

    var body: some View {
        let songs = model.downloadable(tracks)
        if !songs.isEmpty {
            let chosen = model.allChosenForPhone(tracks)
            let here = songs.filter { model.downloadState($0.id) == .downloaded }.count
            Button {
                Task { await model.keepOnPhone(tracks, !chosen) }
            } label: {
                if !chosen {
                    Label("Download Album", systemImage: "arrow.down.circle")
                } else if here < songs.count {
                    Label("Downloading \(here) of \(songs.count)…", systemImage: "arrow.down.circle.dotted")
                } else {
                    Label("Downloaded · Remove Download", systemImage: "arrow.down.circle.fill")
                }
            }
            .accessibilityIdentifier("album.downloadButton")
        }
    }
}

/// A small sign on a song's row: on the phone, coming, or failed.
struct DownloadBadge: View {
    let model: ListenModel
    let track: LibraryTrack

    var body: some View {
        switch model.downloadState(track.id) {
        case .downloaded:
            if track.downloaded {
                Image(systemName: "arrow.down.circle.fill")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel("Downloaded")
            }
        case .downloading:
            ProgressView()
                .controlSize(.mini)
                .accessibilityLabel("Downloading")
        case .waiting:
            Image(systemName: "arrow.down.circle.dotted")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .accessibilityLabel("Waiting to download")
        case .failed:
            Image(systemName: "exclamationmark.circle")
                .font(.caption2)
                .forgeForeground(.warning)
                .accessibilityLabel("Download failed")
        case .notDownloaded:
            EmptyView()
        }
    }
}

/// The songs chosen for the phone: what is here, what is coming, the space
/// it takes, and removing them.
struct DownloadsView: View {
    let model: ListenModel
    let player: TakePlayer

    @State private var details: String?
    @State private var confirmingRemoveAll = false

    private var offline: OfflineSummary { model.offline }

    var body: some View {
        let tracks = offline.tracks.map(\.track)
        let playable = offline.tracks.filter { $0.state == .downloaded }.map(\.track)
        List {
            Section {
                PlayShuffleButtons(disabled: playable.isEmpty) { shuffle in
                    player.play(model.queue(playable), shuffle: shuffle)
                }
            }
            Section {
                ForEach(offline.tracks, id: \.track.id) { item in
                    TrackRow(model: model, player: player, track: item.track,
                             showDetails: { details = item.track.id }) {
                        let index = playable.firstIndex { $0.id == item.track.id }
                        if let index {
                            player.play(model.queue(playable), from: index)
                        } else {
                            player.play(model.queue([item.track]))
                        }
                    }
                }
                .onDelete { places in
                    let gone = places.map { tracks[$0] }
                    Task { await model.keepOnPhone(gone, false) }
                }
            } header: {
                Text(TakeFormat.offlineSummary(offline))
            } footer: {
                if offline.failed > 0 {
                    Text(failure)
                }
            }
            if offline.failed > 0 {
                Section {
                    Button("Try Again", systemImage: "arrow.clockwise") { model.syncDownloads() }
                        .disabled(offline.syncing)
                        .accessibilityIdentifier("downloads.retry")
                }
            }
        }
        .navigationTitle("Downloaded")
        .navigationDestination(item: $details) { id in
            TrackView(model: model, player: player, id: id)
        }
        .toolbar {
            if !offline.tracks.isEmpty {
                ToolbarItem(placement: .primaryAction) {
                    Menu("More", systemImage: "ellipsis.circle") {
                        Button("Remove All Downloads", systemImage: "trash", role: .destructive) {
                            confirmingRemoveAll = true
                        }
                        .accessibilityIdentifier("downloads.removeAll")
                    }
                    .accessibilityIdentifier("downloads.more")
                }
            }
        }
        .confirmationDialog("Remove all downloads?", isPresented: $confirmingRemoveAll, titleVisibility: .visible) {
            Button("Remove All Downloads", role: .destructive) {
                Task { await model.removeAllDownloads() }
            }
            .accessibilityIdentifier("downloads.confirmRemoveAll")
        } message: {
            Text("The songs stay in your library and stream again when you play them.")
        }
        .overlay {
            if offline.tracks.isEmpty {
                ForgeStateView.empty(
                    "Nothing Downloaded", systemImage: "arrow.down.circle",
                    description: "Choose Download on a song, album or playlist to play it without the take server."
                )
            }
        }
        .onAppear { model.syncDownloads() }
        .forgeFeedback(model.feedback)
    }

    /// Why the first failed song failed.
    private var failure: String {
        for item in offline.tracks {
            if case .failed(let reason) = item.state {
                return "Some songs didn't download: \(reason)."
            }
        }
        return ""
    }
}

// MARK: Your own files

/// Import Music: picks audio files in Files and copies them into the
/// library, then says what happened.
struct ImportMusicButton: View {
    let model: ListenModel

    @State private var picking = false
    @State private var importing = false
    @State private var summary: String?

    var body: some View {
        Button("Import Music", systemImage: "square.and.arrow.down") { picking = true }
            .disabled(importing)
            .accessibilityIdentifier("library.import")
            .fileImporter(isPresented: $picking, allowedContentTypes: [.audio], allowsMultipleSelection: true) { result in
                guard case .success(let urls) = result, !urls.isEmpty else { return }
                importing = true
                Task {
                    if let done = await MusicImporter.importFiles(urls, into: model) {
                        summary = TakeFormat.importSummary(done)
                    }
                    importing = false
                }
            }
            .alert("Import Music", isPresented: Binding(
                get: { summary != nil }, set: { if !$0 { summary = nil } }
            )) {
                Button("OK") { summary = nil }
                    .accessibilityIdentifier("import.done")
            } message: {
                Text(summary ?? "")
            }
    }
}
#endif
