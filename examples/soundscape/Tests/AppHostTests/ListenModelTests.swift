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
        XCTAssertEqual(model.phase, .ready)

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
        XCTAssertEqual(model.phase, .ready)
    }

    /// A song field that is too long says which one and the limit, so
    /// Edit Song can explain it (defect hunt finding 4).
    func testTooLongSongFieldNamesTheField() {
        XCTAssertEqual(
            SoundscapeErrors.describe(.OutOfRange(setting: "title")),
            "Keep the title to \(SoundscapeErrors.maxFieldChars) characters or fewer.")
        XCTAssertEqual(SoundscapeErrors.describe(.OutOfRange(setting: "files")), "That is out of range.")
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
        XCTAssertEqual(model.phase, .ready)
    }

    /// Coming back from the background reloads the library through the
    /// host, so picks made on the computer meanwhile show without a pull.
    func testComingBackReloadsTheLibrary() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let model = ListenModel(core: try XCTUnwrap(host.core))
        model.follow(host)
        try await model.setServer("127.0.0.1:9")
        XCTAssertEqual(model.phase, .loading)

        host.report(.background)
        host.report(.active)
        for _ in 0..<500 where model.phase == .loading {
            try await Task.sleep(for: .milliseconds(10))
        }
        // Nothing answers at that address: the reload ran and says so.
        guard case .failed = model.phase else { return XCTFail("not reloaded: \(model.phase)") }
        host.close()
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

        let made = try await model.createPlaylist("  Late drive ")
        XCTAssertEqual(made.playlist.name, "Late drive")
        XCTAssertEqual(model.collection.playlists.map(\.name), ["Late drive"])
        do {
            try await model.renamePlaylist(made.playlist.id, to: "   ")
            XCTFail("an empty name is refused")
        } catch {
            XCTAssertNil(model.alert, "the name sheet shows the refusal, not an alert")
        }
        let renamed = try await model.renamePlaylist(made.playlist.id, to: "Night drive")
        XCTAssertEqual(renamed.playlist.name, "Night drive")
        await model.deletePlaylist(made.playlist.id)
        XCTAssertTrue(model.collection.playlists.isEmpty)
        XCTAssertNil(model.playlist(made.playlist.id))
    }

    func testOwnFilesAndDownloads() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let model = ListenModel(core: try XCTUnwrap(host.core))
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        func file(_ name: String, _ text: String) throws -> MusicFile {
            let url = folder.appendingPathComponent(name)
            try Data(text.utf8).write(to: url)
            return MusicFile(path: url.path, fileName: name, title: nil, artist: "Me", album: nil, number: nil,
                             genre: nil, durationSeconds: 90, artworkPath: nil)
        }
        let resultResult = await model.importMusic([try file("Demo_one.mp3", "a"), try file("x.txt", "b")])
        let result = try XCTUnwrap(resultResult)
        XCTAssertEqual(result.added.count, 1)
        XCTAssertEqual(TakeFormat.importSummary(result),
                       "Added 1 song. 1 couldn't be imported: x.txt: not a music file.")
        let demo = try XCTUnwrap(model.library.tracks.first)
        XCTAssertEqual(demo.title, "Demo one")
        XCTAssertTrue(demo.downloaded)
        XCTAssertEqual(demo.preferred?.kind, .imported)

        // An imported song is on the phone already: nothing to download.
        XCTAssertTrue(model.downloadable([demo]).isEmpty)
        await model.keepOnPhone([demo], true)
        XCTAssertFalse(model.allChosenForPhone([demo]))
        XCTAssertTrue(TakeFormat.offlineSummary(model.offline).hasPrefix("0 songs, "))

        await model.removeImported(demo.id)
        XCTAssertTrue(model.library.tracks.isEmpty)
    }

    func testPlaylistSummary() {
        let list = Playlist(
            id: 1, name: "Mix", trackCount: 12, missingCount: 2, durationSeconds: 2880,
            artworkUrls: [], createdMs: 0, modifiedMs: 0
        )
        XCTAssertEqual(TakeFormat.playlistSummary(list), "12 songs, 48 minutes · 2 not in your library")
        XCTAssertEqual(lyricLines(lyrics: "[chorus]\nOne more night").map(\.text), ["Chorus", "One more night"])
        XCTAssertTrue(countsAsPlayed(listened: 30, length: 200))
        XCTAssertFalse(countsAsPlayed(listened: 10, length: 200))
    }
}
