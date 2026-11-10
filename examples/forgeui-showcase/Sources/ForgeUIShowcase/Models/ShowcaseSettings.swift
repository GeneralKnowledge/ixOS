import ForgeUI
import Foundation

// What the appearance bar switches. Foundation only: the themes are plain
// ForgeUI values, so a Linux test checks they are readable.

public enum ShowcaseTheme: String, CaseIterable, Identifiable, Sendable {
    case standard
    case forest

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .standard: "Standard"
        case .forest: "Forest"
        }
    }

    public var theme: ForgeTheme {
        switch self {
        case .standard: .standard
        case .forest: .showcaseForest
        }
    }
}

extension ForgeTheme {
    /// What an app's own brand looks like: one accent and rounder corners,
    /// changed in one value. Everything else follows.
    public static let showcaseForest = ForgeTheme(
        palette: ForgePalette.standard.withAccent(
            light: ForgeToneColors(
                solid: ForgeRGBA(hex: 0x1F7A4D), onSolid: ForgeRGBA(hex: 0xFFFFFF),
                text: ForgeRGBA(hex: 0x17603C), softOpacity: ForgePalette.lightSoft
            ),
            dark: ForgeToneColors(
                solid: ForgeRGBA(hex: 0x4CD08A), onSolid: ForgeRGBA(hex: 0x000000),
                text: ForgeRGBA(hex: 0x8BE3B3), softOpacity: ForgePalette.darkSoft
            )
        ),
        radius: ForgeRadius(s: 9, m: 18, l: 26)
    )
}

public enum ShowcaseAppearance: String, CaseIterable, Identifiable, Sendable {
    case system
    case light
    case dark

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }
}

public enum ShowcaseTextSize: String, CaseIterable, Identifiable, Sendable {
    case system
    case small
    case large
    case accessibility

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .system: "System"
        case .small: "Small"
        case .large: "Extra Large"
        case .accessibility: "Accessibility"
        }
    }
}
