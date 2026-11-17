// The accents an app can choose from: each a light and a dark tone, already
// checked for contrast (ForgeUITests), so an app gets its own colour without
// picking hex values. Foundation only.
//
// The accent is an app's main way to look like itself: its tint on buttons,
// toggles, links and selection. Keep everything else native (system
// backgrounds, text styles, materials) and let the accent and the app's own
// content carry its personality.

/// A checked accent colour for an app: `.forgeTheme(ForgeTheme(accent: .teal))`.
public enum ForgeAccent: String, CaseIterable, Identifiable, Hashable, Sendable {
    /// ForgeUI's default, close to iOS's own blue.
    case blue
    case teal
    case green
    case indigo
    case purple
    case pink
    case orange
    /// A quiet grey, for tools where colour should come from content.
    case graphite

    public var id: String { rawValue }

    public var name: String {
        switch self {
        case .blue: "Blue"
        case .teal: "Teal"
        case .green: "Green"
        case .indigo: "Indigo"
        case .purple: "Purple"
        case .pink: "Pink"
        case .orange: "Orange"
        case .graphite: "Graphite"
        }
    }

    /// The tone in light mode: a solid dark enough for white text, and a
    /// text colour readable on every light background and on the soft fill.
    public var light: ForgeToneColors {
        let (solid, text): (UInt32, UInt32) = switch self {
        case .blue: (0x0062CC, 0x00519E)
        case .teal: (0x00727A, 0x005E64)
        case .green: (0x2B7A3B, 0x226530)
        case .indigo: (0x4B48C6, 0x3D3AA8)
        case .purple: (0x8A3FBF, 0x7330A3)
        case .pink: (0xC2185B, 0xA3144C)
        case .orange: (0xB04A00, 0x8F3C00)
        case .graphite: (0x5A5A62, 0x46464C)
        }
        return ForgeToneColors(
            solid: ForgeRGBA(hex: solid), onSolid: ForgeRGBA(hex: 0xFFFFFF),
            text: ForgeRGBA(hex: text), softOpacity: ForgePalette.lightSoft
        )
    }

    /// The tone in dark mode: a solid light enough for black text, and a
    /// lighter text colour for dark backgrounds.
    public var dark: ForgeToneColors {
        let (solid, text): (UInt32, UInt32) = switch self {
        case .blue: (0x409CFF, 0x8CC4FF)
        case .teal: (0x30C8D0, 0x7FE0E5)
        case .green: (0x4CC764, 0x8EE09E)
        case .indigo: (0x8583FF, 0xB6B4FF)
        case .purple: (0xC58AF5, 0xDDB8FA)
        case .pink: (0xFF7AA8, 0xFFAFC9)
        case .orange: (0xFF9F45, 0xFFC08A)
        case .graphite: (0xA1A1A8, 0xD1D1D6)
        }
        return ForgeToneColors(
            solid: ForgeRGBA(hex: solid), onSolid: ForgeRGBA(hex: 0x000000),
            text: ForgeRGBA(hex: text), softOpacity: ForgePalette.darkSoft
        )
    }
}

extension ForgePalette {
    /// The same palette with one of ForgeUI's checked accents.
    public func withAccent(_ accent: ForgeAccent) -> ForgePalette {
        withAccent(light: accent.light, dark: accent.dark)
    }
}

extension ForgeTheme {
    /// ForgeUI's standard look with an app's own accent:
    /// `.forgeTheme(ForgeTheme(accent: .teal))`.
    public init(accent: ForgeAccent) {
        self.init(palette: ForgePalette.standard.withAccent(accent))
    }
}
