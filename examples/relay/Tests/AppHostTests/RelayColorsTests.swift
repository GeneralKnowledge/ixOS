import ForgeUI
@testable import RelayApp
import XCTest

/// Relay's colours meet WCAG AA wherever ForgeUI draws them, and the
/// redesign's tokens meet it on the backgrounds they are meant for.
final class RelayColorsTests: XCTestCase {
    func testTheThemeIsReadableEverywhereForgeUIDrawsIt() {
        XCTAssertEqual(RelayColors.theme.contrastIssues().map(\.description), [])
    }

    func testOutgoingBubbleTextReadsOnTheAccent() {
        for tone in [RelayColors.accent, RelayColors.accentDark] {
            XCTAssertGreaterThanOrEqual(ForgeContrast.ratio(tone.onSolid, on: tone.solid), ForgeContrast.text)
        }
    }

    func testTheRedesignTokensReadOnTheirBackgrounds() {
        let grouped = ForgePalette.standard.light.groupedBackground
        let white = ForgeRGBA(hex: 0xFFFFFF)
        let pairs: [(String, ForgeRGBA, ForgeRGBA)] = [
            ("text on midnight", RelayColors.textDark, RelayColors.midnight),
            ("text on surface", RelayColors.textDark, RelayColors.surfaceDark),
            ("secondary on midnight", RelayColors.secondaryDark, RelayColors.midnight),
            ("secondary on surface", RelayColors.secondaryDark, RelayColors.surfaceDark),
            ("secondary on white", RelayColors.secondaryLight, white),
            ("secondary on grouped", RelayColors.secondaryLight, grouped),
            ("accent text on midnight", RelayColors.accentDark.text, RelayColors.midnight),
            ("accent text on surface", RelayColors.accentDark.text, RelayColors.surfaceDark),
        ]
        for (name, fg, bg) in pairs {
            XCTAssertGreaterThanOrEqual(ForgeContrast.ratio(fg, on: bg), ForgeContrast.text, name)
        }
        // Outgoing bubbles as shapes against the redesign's dark background.
        XCTAssertGreaterThanOrEqual(
            ForgeContrast.ratio(RelayColors.accentDark.solid, on: RelayColors.midnight), ForgeContrast.largeTextOrShape)
    }
}
