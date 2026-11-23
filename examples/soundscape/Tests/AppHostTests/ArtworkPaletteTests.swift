@testable import SoundscapeApp
import XCTest

/// Now Playing's colours (ArtworkPalette.swift): white text always reads,
/// and a song with no artwork gets its stand-in's colours.
final class ArtworkPaletteTests: XCTestCase {
    private func pixels(_ colours: [(UInt8, UInt8, UInt8)]) -> [UInt8] {
        colours.flatMap { [$0.0, $0.1, $0.2, 255] }
    }

    func testWhiteTextReadsOverEveryPalette() throws {
        let white = try XCTUnwrap(ArtworkPalette.from(rgba: pixels(Array(repeating: (250, 250, 250), count: 16))))
        XCTAssertGreaterThanOrEqual(white.top.whiteContrast, ArtworkPalette.minimumContrast)
        let yellow = try XCTUnwrap(ArtworkPalette.from(rgba: pixels(Array(repeating: (255, 230, 0), count: 16))))
        XCTAssertGreaterThanOrEqual(yellow.top.whiteContrast, ArtworkPalette.minimumContrast)
        for title in ["Rich Enough to Work", "a", "Ω", ""] {
            XCTAssertGreaterThanOrEqual(ArtworkPalette.standIn(title: title).top.whiteContrast,
                                        ArtworkPalette.minimumContrast)
        }
    }

    func testTheMostColourfulPartLeads() throws {
        // A dark grey cover with a little red in it gives a red player.
        var colours = Array(repeating: (UInt8(30), UInt8(30), UInt8(32)), count: 60)
        colours += Array(repeating: (UInt8(200), UInt8(20), UInt8(20)), count: 4)
        let palette = try XCTUnwrap(ArtworkPalette.from(rgba: pixels(colours)))
        XCTAssertGreaterThan(palette.top.red, palette.top.green * 2)
        XCTAssertLessThan(palette.bottom.luminance, palette.top.luminance)
    }

    func testTransparentOrEmptyPicturesHaveNoPalette() {
        XCTAssertNil(ArtworkPalette.from(rgba: []))
        XCTAssertNil(ArtworkPalette.from(rgba: [255, 0, 0, 0, 0, 255, 0, 0]))
    }

    func testStandInMatchesTheArtworkStandIn() {
        XCTAssertEqual(ArtworkPalette.standInHue("Blackgrass"), ArtworkPalette.standInHue("Blackgrass"))
        XCTAssertTrue((0..<1).contains(ArtworkPalette.standInHue("Blackgrass")))
    }
}
