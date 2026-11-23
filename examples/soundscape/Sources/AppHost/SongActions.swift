#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

// The same actions on a song wherever it shows (docs/design.md, S3.5): a
// row in the library, a search result, a tile on Listen, a catalogue song.

/// What a song's menu needs from the app around it: the radio, moving to
/// a screen, and the Add to Playlist sheet. `ListenView` provides them.
struct SongActions {
    var startRadio: @MainActor (StationSeed, String) -> Void = { _, _ in }
    var go: @MainActor (ListenRoute) -> Void = { _ in }
    var addToPlaylist: @MainActor ([String]) -> Void = { _ in }
}

private struct SongActionsKey: EnvironmentKey {
    // Computed, so there is no shared value to isolate.
    static var defaultValue: SongActions { SongActions() }
}

extension EnvironmentValues {
    var songActions: SongActions {
        get { self[SongActionsKey.self] }
        set { self[SongActionsKey.self] = newValue }
    }
}

/// One song as a menu sees it, from the library or a catalogue.
@MainActor
struct SongRef {
    let id: String
    let title: String
    let artistId: String?
    /// A catalogue song: added to the library before it is kept anywhere.
    let catalogue: CatalogueTrack?
    let queue: [QueueTrack]

    init(_ track: LibraryTrack, model: ListenModel) {
        id = track.id
        title = track.title
        artistId = track.artistId
        catalogue = nil
        queue = model.queue([track])
    }

    init(_ track: CatalogueTrack, model: ListenModel) {
        id = track.id
        title = track.title
        artistId = model.library.tracks.first { $0.id == track.id }?.artistId
        catalogue = track
        queue = model.queue([track])
    }
}

/// Play Next, Add to Queue, Favourite, Add to Playlist, Start Radio and Go
/// to Artist, for a context menu. `identifier` is the screen's prefix.
struct SongMenu: View {
    let model: ListenModel
    let player: TakePlayer
    let song: SongRef
    let identifier: String
    @Environment(\.songActions) private var actions

    var body: some View {
        if !song.queue.isEmpty {
            Button("Play Next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                player.add(song.queue, next: true)
            }
            .accessibilityIdentifier("\(identifier).playNext")
            Button("Add to Queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                player.add(song.queue, next: false)
            }
            .accessibilityIdentifier("\(identifier).addToQueue")
        }
        let favourite = model.isFavourite(song.id)
        Button(favourite ? "Remove from Favourites" : "Add to Favourites",
               systemImage: favourite ? "heart.slash" : "heart") {
            Task {
                await keepInLibrary()
                await model.toggleFavourite(song.id)
            }
        }
        .accessibilityIdentifier("\(identifier).favourite")
        Button("Add to Playlist…", systemImage: "text.badge.plus") {
            Task {
                await keepInLibrary()
                actions.addToPlaylist([song.id])
            }
        }
        .accessibilityIdentifier("\(identifier).addToPlaylist")
        Button("Start Radio", systemImage: "dot.radiowaves.left.and.right") {
            actions.startRadio(.track(id: song.id), "\(song.title) Station")
        }
        .accessibilityIdentifier("\(identifier).startRadio")
        if let artist = libraryArtist {
            Button("Go to Artist", systemImage: "music.mic") { actions.go(.artist(artist)) }
                .accessibilityIdentifier("\(identifier).goToArtist")
        } else if song.id.hasPrefix("jamendo:") {
            Button("More by This Artist", systemImage: "music.mic") {
                actions.go(.jamendoMore(trackId: song.id, by: .artist))
            }
            .accessibilityIdentifier("\(identifier).goToArtist")
        }
    }

    private var libraryArtist: LibraryArtist? {
        guard let artistId = song.artistId else { return nil }
        return model.library.artists.first { $0.id == artistId }
    }

    /// A catalogue song joins the library first, so a favourite or a
    /// playlist can show it.
    private func keepInLibrary() async {
        if let track = song.catalogue, !model.inLibrary(track.id) {
            await model.addToLibrary([track])
        }
    }
}
#endif
