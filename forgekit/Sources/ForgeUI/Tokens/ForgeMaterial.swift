// Which material each kind of surface gets, in one place (docs/forgeui.md,
// "Materials and motion"). Foundation only, so the rules are checked on
// Linux; the SwiftUI side is ForgeMaterial+SwiftUI.swift.
//
// The rule behind the table: Liquid Glass is for controls that float over
// content. Content stays solid. A passive notice (an Undo banner, a
// connection notice) is not a control, so it gets the standard material,
// which is solid under Reduce Transparency.

/// The two kinds of Liquid Glass iOS 26 offers.
public enum ForgeGlassStyle: String, CaseIterable, Hashable, Sendable {
    /// The default: frosted enough to read over anything.
    case regular
    /// Nearly transparent, for small controls over a background already
    /// dark enough for white glyphs (an artwork palette clamped for white
    /// text). Over raw media that can be bright, dim it first.
    case clear
}

/// What a surface is, which decides its material.
public enum ForgeSurfaceKind: String, CaseIterable, Hashable, Sendable {
    /// A control floating over content: a mini-player, a composer, a
    /// floating button.
    case floatingControl
    /// A small control over artwork or video.
    case controlOverMedia
    /// A passive message over content: an Undo banner, a connection notice.
    case notice
    /// A sheet or overlay that lets what opened it show through.
    case overlay
    /// Lists, cards, bubbles, rows: everything a person reads.
    case content
}

/// The material a surface is drawn with.
public enum ForgeSurfaceMaterial: Hashable, Sendable {
    /// Liquid Glass on iOS 26; `fallback` before it.
    case glass(ForgeGlassStyle)
    /// The system's regular material, with a hairline edge and the fallback
    /// shadow.
    case standard
    /// The system's thin material (sheets that show what is under them).
    case thin
    /// An opaque colour from the theme.
    case solid
}

/// The numbers the materials are drawn with.
public enum ForgeMaterial {
    /// The material for a kind of surface.
    public static func material(for kind: ForgeSurfaceKind) -> ForgeSurfaceMaterial {
        switch kind {
        case .floatingControl: .glass(.regular)
        case .controlOverMedia: .glass(.clear)
        case .notice: .standard
        case .overlay: .thin
        case .content: .solid
        }
    }

    /// What a material becomes when Reduce Transparency is on: everything
    /// translucent turns solid. (iOS 26 glass does this by itself; the
    /// fallbacks follow the same rule.)
    public static func reducedTransparency(_ material: ForgeSurfaceMaterial) -> ForgeSurfaceMaterial {
        .solid
    }

    /// The white wash that stands in for clear glass before iOS 26.
    public static let clearGlassFallbackWash = 0.14

    /// The shadow under a floating surface before iOS 26 (glass casts its
    /// own). Shadows exist only in fallbacks and under large artwork.
    public static let fallbackShadow = ForgeShadow(opacity: 0.12, radius: 12, y: 4)
    /// The hairline round a standard-material surface, as a fraction of the
    /// theme's separator colour.
    public static let hairlineOpacity = 0.6
    public static let hairlineWidth = 0.5
}

/// A black drop shadow.
public struct ForgeShadow: Hashable, Sendable {
    public var opacity: Double
    public var radius: Double
    public var y: Double

    public init(opacity: Double, radius: Double, y: Double) {
        self.opacity = opacity
        self.radius = radius
        self.y = y
    }
}
