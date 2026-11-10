// WCAG 2 contrast, and a check of every colour pairing ForgeUI's components
// draw. Foundation only: run it in a Linux test, or on a custom theme at
// runtime (the showcase does).

import Foundation

public enum ForgeContrast {
    /// Body text (WCAG AA).
    public static let text = 4.5
    /// Large text (18 pt, or 14 pt bold) and shapes that carry meaning (WCAG AA).
    public static let largeTextOrShape = 3.0

    /// Relative luminance of an opaque colour, 0 (black) to 1 (white).
    public static func luminance(_ color: ForgeRGBA) -> Double {
        func linear(_ c: Double) -> Double {
            c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(color.red) + 0.7152 * linear(color.green) + 0.0722 * linear(color.blue)
    }

    /// The contrast ratio of two colours, 1 to 21. A translucent foreground
    /// is first drawn over the background.
    public static func ratio(_ foreground: ForgeRGBA, on background: ForgeRGBA) -> Double {
        let fg = foreground.alpha < 1 ? foreground.composited(over: background) : foreground
        let a = luminance(fg)
        let b = luminance(background)
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }
}

/// One pairing in a theme that falls below its contrast threshold.
public struct ForgeContrastIssue: Hashable, Sendable, CustomStringConvertible {
    public var appearance: String
    public var pairing: String
    public var ratio: Double
    public var minimum: Double

    public var description: String {
        let shown = (ratio * 100).rounded() / 100
        return "\(appearance): \(pairing) is \(shown):1, needs \(minimum):1"
    }
}

extension ForgeTheme {
    /// Every foreground/background pairing ForgeUI's components draw, in
    /// light and dark, that falls below WCAG AA. Empty means the theme is
    /// readable everywhere ForgeUI puts text or meaningful shapes.
    public func contrastIssues() -> [ForgeContrastIssue] {
        var issues: [ForgeContrastIssue] = []
        for (appearance, colors) in [("light", palette.light), ("dark", palette.dark)] {
            func check(_ pairing: String, _ fg: ForgeRGBA, _ bg: ForgeRGBA, _ minimum: Double) {
                let ratio = ForgeContrast.ratio(fg, on: bg)
                if ratio < minimum {
                    issues.append(ForgeContrastIssue(appearance: appearance, pairing: pairing, ratio: ratio, minimum: minimum))
                }
            }
            for (name, bg) in colors.backgrounds {
                check("textPrimary on \(name)", colors.textPrimary, bg, ForgeContrast.text)
                check("textSecondary on \(name)", colors.textSecondary, bg, ForgeContrast.text)
                check("textTertiary on \(name)", colors.textTertiary, bg, ForgeContrast.text)
                for tone in ForgeTone.allCases {
                    let t = colors.tone(tone)
                    // Tertiary buttons, links, tinted labels.
                    check("\(tone) text on \(name)", t.text, bg, ForgeContrast.text)
                    // Badges, secondary buttons: text on the soft fill over this background.
                    check("\(tone) text on \(tone) soft over \(name)", t.text, t.soft.composited(over: bg), ForgeContrast.text)
                    // Status dots, icon tiles, filled buttons as shapes.
                    check("\(tone) solid against \(name)", t.solid, bg, ForgeContrast.largeTextOrShape)
                }
            }
            for tone in ForgeTone.allCases {
                let t = colors.tone(tone)
                // Filled buttons, icons on tiles.
                check("\(tone) onSolid on \(tone) solid", t.onSolid, t.solid, ForgeContrast.text)
            }
        }
        return issues
    }
}
