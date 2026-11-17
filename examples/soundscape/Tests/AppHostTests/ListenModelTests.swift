@testable import SoundscapeApp
@preconcurrency import SoundscapeFFI
import XCTest

/// Listen's model against the real Rust core: the address rules and the
/// words on screen. Talking to a take server is covered in Rust
/// (`rust/tests/listen.rs`) and on the phone.
@MainActor
final class ListenModelTests: XCTestCase {
    func testServerAddress() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let model = ListenModel(core: try XCTUnwrap(host.core))
        XCTAssertNil(model.server)
        XCTAssertEqual(model.phase, .needsServer)

        // A host name over plain http is refused with a reason to show.
        do {
            try await model.setServer("create")
            XCTFail("a host name should be refused")
        } catch {
            XCTAssertTrue(ListenModel.message(error).contains("IP address"))
        }
        XCTAssertNil(model.server)

        try await model.setServer("100.64.0.7")
        XCTAssertEqual(model.server, "http://100.64.0.7:8642")
        XCTAssertEqual(model.phase, .loading)

        model.askToForgetServer()
        XCTAssertNotNil(model.alert)
        await model.handle(.forgetServer)
        XCTAssertNil(model.server)
        XCTAssertEqual(model.phase, .needsServer)
    }

    func testFormatting() {
        let take = Take(
            id: "a", label: "seed 1", audioUrl: "http://x/a.mp3", seed: 1, model: "xl-turbo", bpm: 92,
            key: "D minor", caption: nil, lyrics: nil, lufs: -14.25, stars: 0, note: "", picked: false, madeMs: 0
        )
        XCTAssertEqual(TakeFormat.details(take), "92 BPM · D minor · xl-turbo")
        XCTAssertEqual(TakeFormat.loudness(-14.25), "−14.3 LUFS")
        XCTAssertNil(TakeFormat.loudness(nil))
        XCTAssertEqual(TakeFormat.time(125.9), "2:05")
        XCTAssertEqual(TakeFormat.match(loudnessMatch(aLufs: -11, bLufs: -14.2)), "A turned down 3.2 dB to match")
        XCTAssertEqual(TakeFormat.match(loudnessMatch(aLufs: -14, bLufs: -14)), "Same loudness")
        XCTAssertFalse(loudnessMatch(aLufs: nil, bLufs: -14).matched)

        let band = LibraryArtist(id: "artist:lowbridge", name: "LOWBRIDGE", artworkUrl: nil, albumCount: 1, trackCount: 3)
        XCTAssertEqual(TakeFormat.artistSummary(band), "1 album · 3 songs")
        XCTAssertEqual(TakeFormat.recordings(1), nil)
        XCTAssertEqual(TakeFormat.recordings(4), "4 recordings")
    }

    func testLibraryStartsEmptyWithoutAServer() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let model = ListenModel(core: try XCTUnwrap(host.core))
        XCTAssertTrue(model.library.tracks.isEmpty)
        XCTAssertTrue(model.search("anything").isEmpty)
        await model.loadLibrary()
        XCTAssertEqual(model.phase, .needsServer)
    }

    func testFavouritesAndPlaylists() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let model = ListenModel(core: try XCTUnwrap(host.core))
        XCTAssertTrue(model.collection.playlists.isEmpty)

        await model.toggleFavourite("album:after hours/after hours")
        XCTAssertTrue(model.isFavourite("album:after hours/after hours"))
        await model.toggleFavourite("album:after hours/after hours")
        XCTAssertFalse(model.isFavourite("album:after hours/after hours"))

        let made = try XCTUnwrap(await model.createPlaylist("  Late drive "))
        XCTAssertEqual(made.playlist.name, "Late drive")
        XCTAssertEqual(model.collection.playlists.map(\.name), ["Late drive"])
        XCTAssertNil(await model.renamePlaylist(made.playlist.id, to: "   "), "an empty name is refused")
        XCTAssertNotNil(model.alert)
        model.alert = nil
        let renamed = try XCTUnwrap(await model.renamePlaylist(made.playlist.id, to: "Night drive"))
        XCTAssertEqual(renamed.playlist.name, "Night drive")
        await model.deletePlaylist(made.playlist.id)
        XCTAssertTrue(model.collection.playlists.isEmpty)
        XCTAssertNil(model.playlist(made.playlist.id))
    }

    func testPlaylistSummary() {
        let list = Playlist(
            id: 1, name: "Mix", trackCount: 12, missingCount: 2, durationSeconds: 2880,
            artworkUrls: [], createdMs: 0, modifiedMs: 0
        )
        XCTAssertEqual(TakeFormat.playlistSummary(list), "12 songs, 48 minutes · 2 not in your library")
        XCTAssertTrue(countsAsPlayed(listened: 30, length: 200))
        XCTAssertFalse(countsAsPlayed(listened: 10, length: 200))
    }
}
