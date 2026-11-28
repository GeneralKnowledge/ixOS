#if canImport(SwiftUI)
import SwiftUI

// The SwiftUI side of ForgeMaterial: Liquid Glass for controls that float
// over content, the standard material for passive notices. One copy of the
// iOS 26 checks, so apps don't each carry their own.
//
// The compiler checks keep this building with Xcode 16 (iOS 18 SDK); the
// availability checks keep iOS 17 to 25 on the look they had before.

/// Whether this build and this device draw Liquid Glass: an iOS 26 SDK
/// and iOS 26 or later. For layouts that differ around a glass surface
/// (a floating bar inset from the edges only when it is glass).
public enum ForgeGlassSupport {
    public static var isAvailable: Bool {
        #if compiler(>=6.2)
        if #available(iOS 26.0, macOS 26.0, *) { return true }
        #endif
        return false
    }
}

extension View {
    /// Liquid Glass for a control that floats over content: a mini-player,
    /// a composer (`.regular`), or a small button over artwork (`.clear`).
    /// Before iOS 26: the regular material and the fallback shadow, or for
    /// clear glass a faint white wash. Under Reduce Transparency the
    /// fallbacks turn solid, as iOS 26 glass does by itself.
    ///
    /// Clear glass is only for a background already dark enough for white
    /// glyphs (Soundscape's artwork palette is clamped to 4.5:1). Over raw
    /// media that can be bright, add a dim behind it first.
    public func forgeGlass<S: Shape>(
        _ style: ForgeGlassStyle = .regular,
        interactive: Bool = false,
        in shape: S
    ) -> some View {
        modifier(ForgeGlassModifier(style: style, interactive: interactive, shape: shape))
    }

    /// The standard material, for a passive surface over content (an Undo
    /// banner, a connection notice): never glass, which is for controls.
    /// Regular material with a hairline edge and the fallback shadow; the
    /// theme's raised surface under Reduce Transparency.
    public func forgeMaterial<S: InsettableShape>(in shape: S) -> some View {
        modifier(ForgeMaterialModifier(shape: shape))
    }

    /// A tinted action floating over content (New Messages): tinted glass
    /// on iOS 26, the bordered prominent style before.
    @ViewBuilder public func forgeGlassProminentButton() -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, macOS 26.0, *) {
            buttonStyle(.glassProminent)
        } else {
            buttonStyle(.borderedProminent)
        }
        #else
        buttonStyle(.borderedProminent)
        #endif
    }
}

private struct ForgeGlassModifier<S: Shape>: ViewModifier {
    let style: ForgeGlassStyle
    let interactive: Bool
    let shape: S
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.forgeTheme) private var theme

    @ViewBuilder func body(content: Content) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, macOS 26.0, *) {
            glass(content)
        } else {
            fallback(content)
        }
        #else
        fallback(content)
        #endif
    }

    #if compiler(>=6.2)
    @available(iOS 26.0, macOS 26.0, *)
    private func glass(_ content: Content) -> some View {
        let base: Glass = style == .clear ? .clear : .regular
        return content.glassEffect(base.interactive(interactive), in: shape)
    }
    #endif

    @ViewBuilder private func fallback(_ content: Content) -> some View {
        let shadow = ForgeMaterial.fallbackShadow
        switch (style, reduceTransparency) {
        case (.regular, false):
            content
                .background(.regularMaterial, in: shape)
                .shadow(color: .black.opacity(shadow.opacity), radius: shadow.radius, y: shadow.y)
        case (.regular, true):
            content
                .background(theme.colors.surfaceRaised, in: shape)
                .shadow(color: .black.opacity(shadow.opacity), radius: shadow.radius, y: shadow.y)
        case (.clear, false):
            content.background(Color.white.opacity(ForgeMaterial.clearGlassFallbackWash), in: shape)
        case (.clear, true):
            // Solid dark, so white glyphs keep their contrast.
            content.background(Color(white: 0.18), in: shape)
        }
    }
}

private struct ForgeMaterialModifier<S: InsettableShape>: ViewModifier {
    let shape: S
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.forgeTheme) private var theme

    func body(content: Content) -> some View {
        let shadow = ForgeMaterial.fallbackShadow
        return content
            .background(
                reduceTransparency
                    ? AnyShapeStyle(theme.colors.surfaceRaised)
                    : AnyShapeStyle(.regularMaterial),
                in: shape
            )
            .overlay(shape.strokeBorder(
                theme.colors.separator.opacity(ForgeMaterial.hairlineOpacity),
                lineWidth: ForgeMaterial.hairlineWidth
            ))
            .shadow(color: .black.opacity(shadow.opacity), radius: shadow.radius, y: shadow.y)
    }
}
#endif
