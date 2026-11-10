import ForgeUI
import XCTest

/// ForgeUI's tokens are plain values, so they are checked here on Linux:
/// every pairing the components draw is readable (WCAG AA), and the scales
/// are in order. The SwiftUI side is compiled on macOS CI and with xtool.
final class ForgeThemeTests: XCTestCase {
    func testContrastMathMatchesKnownValues() {
        let black = ForgeRGBA(hex: 0x000000)
        let white = ForgeRGBA(hex: 0xFFFFFF)
        XCTAssertEqual(ForgeContrast.ratio(black, on: white), 21, accuracy: 0.001)
        XCTAssertEqual(ForgeContrast.ratio(white, on: white), 1, accuracy: 0.001)
        // Order doesn't matter.
        XCTAssertEqual(ForgeContrast.ratio(white, on: black), 21, accuracy: 0.001)
        // #767676 on white is the classic 4.54:1, just over AA.
        XCTAssertEqual(ForgeContrast.ratio(ForgeRGBA(hex: 0x767676), on: white), 4.54, accuracy: 0.01)
        // A translucent colour is judged as drawn: black at 50% over white is mid grey.
        let halfBlack = black.opacity(0.5)
        XCTAssertEqual(
            ForgeContrast.ratio(halfBlack, on: white),
            ForgeContrast.ratio(ForgeRGBA(red: 0.5, green: 0.5, blue: 0.5), on: white),
            accuracy: 0.001
        )
    }

    func testStandardThemeIsReadableInLightAndDark() {
        let issues = ForgeTheme.standard.contrastIssues()
        XCTAssertTrue(issues.isEmpty, issues.map(\.description).joined(separator: "\n"))
    }

    func testTheCheckCatchesAnUnreadableAccent() {
        // Apple's pure yellow as an accent: white text on it is unreadable.
        let yellow = ForgeToneColors(
            solid: ForgeRGBA(hex: 0xFFCC00), onSolid: ForgeRGBA(hex: 0xFFFFFF),
            text: ForgeRGBA(hex: 0xFFCC00), softOpacity: 0.12
        )
        let theme = ForgeTheme(palette: ForgePalette.standard.withAccent(light: yellow, dark: yellow))
        let issues = theme.contrastIssues()
        XCTAssertTrue(issues.contains { $0.appearance == "light" && $0.pairing == "accent onSolid on accent solid" })
        XCTAssertTrue(issues.contains { $0.appearance == "light" && $0.pairing == "accent text on background" })
    }

    func testScalesAscend() {
        let spacing = ForgeSpacing.standard.steps
        XCTAssertEqual(spacing, spacing.sorted())
        XCTAssertEqual(Set(spacing).count, spacing.count)
        let radius = ForgeRadius.standard.steps
        XCTAssertEqual(radius, radius.sorted())
    }

    func testEveryToneResolvesInBothAppearances() {
        for tone in ForgeTone.allCases {
            for colors in [ForgePalette.standard.light, ForgePalette.standard.dark] {
                let t = colors.tone(tone)
                XCTAssertEqual(t.solid.alpha, 1, "\(tone) solid must be opaque")
                XCTAssertLessThan(t.soft.alpha, 1, "\(tone) soft must be a tint")
            }
        }
    }

    func testFieldMessagesMapToTones() {
        XCTAssertEqual(ForgeFieldMessage.error("Required").tone, .danger)
        XCTAssertTrue(ForgeFieldMessage.error("Required").isError)
        XCTAssertEqual(ForgeFieldMessage.help("Shown on the list").tone, .neutral)
        XCTAssertFalse(ForgeFieldMessage.warning("Long").isError)
    }
}
