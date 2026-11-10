// ForgeUI's theme: every token an app can change, in one value. Foundation
// only, so a theme can be checked on Linux (`contrastIssues()`).
//
// There is no typography token: components use the system text styles
// (.body, .headline, ...), so Dynamic Type, Bold Text and new iOS styles
// apply without any work.

/// Spacing steps in points. Components pad and space with these; an app
/// changes the whole rhythm by changing them.
public struct ForgeSpacing: Hashable, Sendable {
    public var xxs: Double
    public var xs: Double
    public var s: Double
    public var m: Double
    public var l: Double
    public var xl: Double
    public var xxl: Double

    public init(xxs: Double = 2, xs: Double = 4, s: Double = 8, m: Double = 12, l: Double = 16, xl: Double = 24, xxl: Double = 32) {
        self.xxs = xxs
        self.xs = xs
        self.s = s
        self.m = m
        self.l = l
        self.xl = xl
        self.xxl = xxl
    }

    public static let standard = ForgeSpacing()

    /// Smallest to largest, for checks.
    public var steps: [Double] { [xxs, xs, s, m, l, xl, xxl] }
}

/// Corner radii in points. Corners are drawn continuous (Apple's squircle).
public struct ForgeRadius: Hashable, Sendable {
    /// Badges' inner shapes, icon tiles.
    public var s: Double
    /// Buttons and fields.
    public var m: Double
    /// Cards and sheets.
    public var l: Double

    public init(s: Double = 7, m: Double = 12, l: Double = 18) {
        self.s = s
        self.m = m
        self.l = l
    }

    public static let standard = ForgeRadius()

    public var steps: [Double] { [s, m, l] }
}

/// Everything about how a ForgeUI app looks. Set it once at the root with
/// `.forgeTheme(_:)`; change the look of the whole app by changing it.
public struct ForgeTheme: Hashable, Sendable {
    public var palette: ForgePalette
    public var spacing: ForgeSpacing
    public var radius: ForgeRadius
    /// Cards cast a soft shadow in light mode (dark mode separates surfaces
    /// by colour instead, as iOS does).
    public var cardShadow: Bool

    public init(
        palette: ForgePalette = .standard,
        spacing: ForgeSpacing = .standard,
        radius: ForgeRadius = .standard,
        cardShadow: Bool = true
    ) {
        self.palette = palette
        self.spacing = spacing
        self.radius = radius
        self.cardShadow = cardShadow
    }

    /// ForgeUI's default look.
    public static let standard = ForgeTheme()
}
