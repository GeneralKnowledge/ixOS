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

    func testSecondaryTextReadsOverEveryPalette() throws {
        // The artist line is white at `secondaryTextOpacity`, which reads
        // less well than opaque white; the palette must allow for it, over
        // both stops, on the brightest covers there are.
        let opacity = ArtworkPalette.secondaryTextOpacity
        let covers: [(UInt8, UInt8, UInt8)] = [(250, 250, 250), (255, 230, 0), (0, 255, 255), (120, 255, 120)]
        for cover in covers {
            let palette = try XCTUnwrap(ArtworkPalette.from(rgba: pixels(Array(repeating: cover, count: 16))))
            XCTAssertGreaterThanOrEqual(palette.top.whiteContrast(atOpacity: opacity), ArtworkPalette.minimumContrast)
            XCTAssertGreaterThanOrEqual(palette.bottom.whiteContrast(atOpacity: opacity), ArtworkPalette.minimumContrast)
        }
        for title in ["Rich Enough to Work", "a", "Ω", ""] {
            let palette = ArtworkPalette.standIn(title: title)
            XCTAssertGreaterThanOrEqual(palette.top.whiteContrast(atOpacity: opacity), ArtworkPalette.minimumContrast)
        }
    }

    func testOpaqueWhiteContrastIsUnchanged() {
        let grey = PaletteColor(red: 0.5, green: 0.5, blue: 0.5)
        XCTAssertEqual(grey.whiteContrast, grey.whiteContrast(atOpacity: 1), accuracy: 1e-12)
        XCTAssertGreaterThan(grey.whiteContrast, grey.whiteContrast(atOpacity: 0.6))
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
