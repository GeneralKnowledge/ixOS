#if canImport(SwiftUI)
import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

// How the theme reaches views: a plain value in the SwiftUI environment
// (no singleton, no observable store). Colours are dynamic, so they follow
// light and dark mode wherever they are drawn, sheets and menus included.

private struct ForgeThemeKey: EnvironmentKey {
    static let defaultValue = ForgeTheme.standard
}

extension EnvironmentValues {
    /// The ForgeUI theme for this part of the view tree.
    public var forgeTheme: ForgeTheme {
        get { self[ForgeThemeKey.self] }
        set { self[ForgeThemeKey.self] = newValue }
    }
}

extension View {
    /// Applies a ForgeUI theme to this view and everything inside it, and
    /// tints plain SwiftUI controls (toggles, pickers, links) with its accent.
    /// Put it once at the root of the app.
    public func forgeTheme(_ theme: ForgeTheme) -> some View {
        environment(\.forgeTheme, theme)
            .tint(theme.colors.accent.solid)
    }
}

extension ForgeRGBA {
    var sRGBColor: Color {
        Color(.sRGB, red: red, green: green, blue: blue, opacity: alpha)
    }

    #if canImport(UIKit)
    var platformColor: UIColor {
        UIColor(red: red, green: green, blue: blue, alpha: alpha)
    }
    #elseif canImport(AppKit)
    var platformColor: NSColor {
        NSColor(srgbRed: red, green: green, blue: blue, alpha: alpha)
    }
    #endif
}

/// A colour that is `light` in light mode and `dark` in dark mode.
func forgeDynamicColor(light: ForgeRGBA, dark: ForgeRGBA) -> Color {
    #if canImport(UIKit)
    let lightColor = light.platformColor
    let darkColor = dark.platformColor
    return Color(uiColor: UIColor { traits in
        traits.userInterfaceStyle == .dark ? darkColor : lightColor
    })
    #elseif canImport(AppKit)
    let lightColor = light.platformColor
    let darkColor = dark.platformColor
    return Color(nsColor: NSColor(name: nil) { appearance in
        appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? darkColor : lightColor
    })
    #else
    return light.sRGBColor
    #endif
}

/// One tone's colours as SwiftUI colours.
public struct ForgeToneStyle {
    public let solid: Color
    public let onSolid: Color
    public let soft: Color
    public let text: Color
}

/// The theme's colours as SwiftUI colours that follow light and dark mode.
/// Read them with `@Environment(\.forgeTheme) var theme` and `theme.colors`.
public struct ForgeThemeColors {
    let palette: ForgePalette

    private func color(_ role: KeyPath<ForgeColors, ForgeRGBA>) -> Color {
        forgeDynamicColor(light: palette.light[keyPath: role], dark: palette.dark[keyPath: role])
    }

    public var background: Color { color(\.background) }
    public var groupedBackground: Color { color(\.groupedBackground) }
    public var surface: Color { color(\.surface) }
    public var surfaceRaised: Color { color(\.surfaceRaised) }
    public var textPrimary: Color { color(\.textPrimary) }
    public var textSecondary: Color { color(\.textSecondary) }
    public var textTertiary: Color { color(\.textTertiary) }
    public var separator: Color { color(\.separator) }
    public var accent: ForgeToneStyle { tone(.accent) }

    public func tone(_ tone: ForgeTone) -> ForgeToneStyle {
        let light = palette.light.tone(tone)
        let dark = palette.dark.tone(tone)
        return ForgeToneStyle(
            solid: forgeDynamicColor(light: light.solid, dark: dark.solid),
            onSolid: forgeDynamicColor(light: light.onSolid, dark: dark.onSolid),
            soft: forgeDynamicColor(light: light.soft, dark: dark.soft),
            text: forgeDynamicColor(light: light.text, dark: dark.text)
        )
    }
}

extension ForgeTheme {
    public var colors: ForgeThemeColors { ForgeThemeColors(palette: palette) }
}

extension View {
    /// The theme's grouped background behind a `List` or `Form`, in place of
    /// the system grey.
    public func forgeGroupedBackground() -> some View {
        modifier(ForgeGroupedBackground())
    }
}

private struct ForgeGroupedBackground: ViewModifier {
    @Environment(\.forgeTheme) private var theme

    func body(content: Content) -> some View {
        content
            .scrollContentBackground(.hidden)
            .background(theme.colors.groupedBackground.ignoresSafeArea())
    }
}
#endif
