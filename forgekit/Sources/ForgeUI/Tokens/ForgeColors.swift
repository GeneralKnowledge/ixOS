// ForgeUI's colour tokens. Foundation only: these are plain values, so a
// theme can be checked (and tested) on Linux. Theme/ForgeTheme+SwiftUI.swift
// turns them into SwiftUI colours that follow light and dark mode.

/// One colour in sRGB, components 0...1.
public struct ForgeRGBA: Hashable, Sendable {
    public var red: Double
    public var green: Double
    public var blue: Double
    public var alpha: Double

    public init(red: Double, green: Double, blue: Double, alpha: Double = 1) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    /// `0xRRGGBB`, for example `ForgeRGBA(hex: 0x0062CC)`.
    public init(hex: UInt32, alpha: Double = 1) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            alpha: alpha
        )
    }

    /// The same colour at another opacity.
    public func opacity(_ alpha: Double) -> ForgeRGBA {
        ForgeRGBA(red: red, green: green, blue: blue, alpha: alpha)
    }

    /// This colour drawn over an opaque `background`: what the eye sees.
    public func composited(over background: ForgeRGBA) -> ForgeRGBA {
        ForgeRGBA(
            red: red * alpha + background.red * (1 - alpha),
            green: green * alpha + background.green * (1 - alpha),
            blue: blue * alpha + background.blue * (1 - alpha)
        )
    }
}

/// The colours of one tone (accent, success, warning, danger, info, neutral).
///
/// `solid` fills a shape (an icon tile, a status dot, a destructive button)
/// and `onSolid` is text or a symbol on it. `soft` is a tinted background
/// (a badge, a secondary button) and `text` is the tone's readable colour on
/// `soft` or on any surface.
public struct ForgeToneColors: Hashable, Sendable {
    public var solid: ForgeRGBA
    public var onSolid: ForgeRGBA
    public var soft: ForgeRGBA
    public var text: ForgeRGBA

    public init(solid: ForgeRGBA, onSolid: ForgeRGBA, soft: ForgeRGBA, text: ForgeRGBA) {
        self.solid = solid
        self.onSolid = onSolid
        self.soft = soft
        self.text = text
    }

    /// A tone whose soft background is `solid` at `softOpacity`.
    public init(solid: ForgeRGBA, onSolid: ForgeRGBA, text: ForgeRGBA, softOpacity: Double) {
        self.init(solid: solid, onSolid: onSolid, soft: solid.opacity(softOpacity), text: text)
    }
}

/// The semantic tones components take, for example `ForgeBadge("Late", tone: .warning)`.
public enum ForgeTone: String, CaseIterable, Identifiable, Hashable, Sendable {
    public var id: String { rawValue }

    case accent
    case neutral
    case success
    case warning
    case danger
    case info
}

/// Every colour role in one appearance (light or dark).
public struct ForgeColors: Hashable, Sendable {
    /// Behind plain screens.
    public var background: ForgeRGBA
    /// Behind grouped lists and forms (iOS Settings' grey).
    public var groupedBackground: ForgeRGBA
    /// Rows, fields and cards on a grouped background.
    public var surface: ForgeRGBA
    /// Cards and sheets that sit above a surface.
    public var surfaceRaised: ForgeRGBA
    public var textPrimary: ForgeRGBA
    public var textSecondary: ForgeRGBA
    public var textTertiary: ForgeRGBA
    /// Hairlines between rows. Decorative, so not held to text contrast.
    public var separator: ForgeRGBA
    public var accent: ForgeToneColors
    public var neutral: ForgeToneColors
    public var success: ForgeToneColors
    public var warning: ForgeToneColors
    public var danger: ForgeToneColors
    public var info: ForgeToneColors

    public init(
        background: ForgeRGBA,
        groupedBackground: ForgeRGBA,
        surface: ForgeRGBA,
        surfaceRaised: ForgeRGBA,
        textPrimary: ForgeRGBA,
        textSecondary: ForgeRGBA,
        textTertiary: ForgeRGBA,
        separator: ForgeRGBA,
        accent: ForgeToneColors,
        neutral: ForgeToneColors,
        success: ForgeToneColors,
        warning: ForgeToneColors,
        danger: ForgeToneColors,
        info: ForgeToneColors
    ) {
        self.background = background
        self.groupedBackground = groupedBackground
        self.surface = surface
        self.surfaceRaised = surfaceRaised
        self.textPrimary = textPrimary
        self.textSecondary = textSecondary
        self.textTertiary = textTertiary
        self.separator = separator
        self.accent = accent
        self.neutral = neutral
        self.success = success
        self.warning = warning
        self.danger = danger
        self.info = info
    }

    public func tone(_ tone: ForgeTone) -> ForgeToneColors {
        switch tone {
        case .accent: accent
        case .neutral: neutral
        case .success: success
        case .warning: warning
        case .danger: danger
        case .info: info
        }
    }

    /// Every opaque background text may sit on.
    public var backgrounds: [(name: String, color: ForgeRGBA)] {
        [
            ("background", background),
            ("groupedBackground", groupedBackground),
            ("surface", surface),
            ("surfaceRaised", surfaceRaised),
        ]
    }
}

/// A light and a dark set of colours.
public struct ForgePalette: Hashable, Sendable {
    public var light: ForgeColors
    public var dark: ForgeColors

    public init(light: ForgeColors, dark: ForgeColors) {
        self.light = light
        self.dark = dark
    }

    public func colors(dark isDark: Bool) -> ForgeColors {
        isDark ? dark : light
    }

    /// The same palette with another accent, for example a brand colour.
    /// Check the result with `ForgeTheme.contrastIssues()`.
    public func withAccent(light lightAccent: ForgeToneColors, dark darkAccent: ForgeToneColors) -> ForgePalette {
        var copy = self
        copy.light.accent = lightAccent
        copy.dark.accent = darkAccent
        return copy
    }
}

extension ForgePalette {
    /// ForgeUI's default palette: close to iOS's own system colours, with
    /// text and tone colours adjusted where Apple's fall below WCAG AA
    /// contrast (4.5:1) on the backgrounds they are used on. Checked by
    /// ForgeUITests on every change.
    public static let standard = ForgePalette(
        light: ForgeColors(
            background: ForgeRGBA(hex: 0xFFFFFF),
            groupedBackground: ForgeRGBA(hex: 0xF2F2F7),
            surface: ForgeRGBA(hex: 0xFFFFFF),
            surfaceRaised: ForgeRGBA(hex: 0xFFFFFF),
            textPrimary: ForgeRGBA(hex: 0x000000),
            textSecondary: ForgeRGBA(hex: 0x55555D),
            textTertiary: ForgeRGBA(hex: 0x6E6E76),
            separator: ForgeRGBA(hex: 0xC6C6C8),
            accent: ForgeToneColors(solid: ForgeRGBA(hex: 0x0062CC), onSolid: ForgeRGBA(hex: 0xFFFFFF), text: ForgeRGBA(hex: 0x00519E), softOpacity: ForgePalette.lightSoft),
            neutral: ForgeToneColors(solid: ForgeRGBA(hex: 0x6E6E76), onSolid: ForgeRGBA(hex: 0xFFFFFF), text: ForgeRGBA(hex: 0x3A3A40), softOpacity: ForgePalette.lightSoft),
            success: ForgeToneColors(solid: ForgeRGBA(hex: 0x1E7D34), onSolid: ForgeRGBA(hex: 0xFFFFFF), text: ForgeRGBA(hex: 0x1A6B2C), softOpacity: ForgePalette.lightSoft),
            warning: ForgeToneColors(solid: ForgeRGBA(hex: 0xB25000), onSolid: ForgeRGBA(hex: 0xFFFFFF), text: ForgeRGBA(hex: 0x8F4000), softOpacity: ForgePalette.lightSoft),
            danger: ForgeToneColors(solid: ForgeRGBA(hex: 0xD70015), onSolid: ForgeRGBA(hex: 0xFFFFFF), text: ForgeRGBA(hex: 0xB00012), softOpacity: ForgePalette.lightSoft),
            info: ForgeToneColors(solid: ForgeRGBA(hex: 0x0071A4), onSolid: ForgeRGBA(hex: 0xFFFFFF), text: ForgeRGBA(hex: 0x005A84), softOpacity: ForgePalette.lightSoft)
        ),
        dark: ForgeColors(
            background: ForgeRGBA(hex: 0x000000),
            groupedBackground: ForgeRGBA(hex: 0x000000),
            surface: ForgeRGBA(hex: 0x1C1C1E),
            surfaceRaised: ForgeRGBA(hex: 0x2C2C2E),
            textPrimary: ForgeRGBA(hex: 0xFFFFFF),
            textSecondary: ForgeRGBA(hex: 0xAEAEB2),
            textTertiary: ForgeRGBA(hex: 0x98989F),
            separator: ForgeRGBA(hex: 0x38383A),
            accent: ForgeToneColors(solid: ForgeRGBA(hex: 0x409CFF), onSolid: ForgeRGBA(hex: 0x000000), text: ForgeRGBA(hex: 0x8CC4FF), softOpacity: ForgePalette.darkSoft),
            neutral: ForgeToneColors(solid: ForgeRGBA(hex: 0x8E8E93), onSolid: ForgeRGBA(hex: 0x000000), text: ForgeRGBA(hex: 0xD1D1D6), softOpacity: ForgePalette.darkSoft),
            success: ForgeToneColors(solid: ForgeRGBA(hex: 0x30D158), onSolid: ForgeRGBA(hex: 0x000000), text: ForgeRGBA(hex: 0x4CE07A), softOpacity: ForgePalette.darkSoft),
            warning: ForgeToneColors(solid: ForgeRGBA(hex: 0xFF9F0A), onSolid: ForgeRGBA(hex: 0x000000), text: ForgeRGBA(hex: 0xFFB340), softOpacity: ForgePalette.darkSoft),
            danger: ForgeToneColors(solid: ForgeRGBA(hex: 0xFF6961), onSolid: ForgeRGBA(hex: 0x000000), text: ForgeRGBA(hex: 0xFFA19C), softOpacity: ForgePalette.darkSoft),
            info: ForgeToneColors(solid: ForgeRGBA(hex: 0x64D2FF), onSolid: ForgeRGBA(hex: 0x000000), text: ForgeRGBA(hex: 0xA0E4FF), softOpacity: ForgePalette.darkSoft)
        )
    )

    /// Soft tone backgrounds are the solid colour at this opacity.
    public static let lightSoft = 0.12
    public static let darkSoft = 0.24
}
