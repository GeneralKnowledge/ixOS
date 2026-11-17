#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Styles text by what it is (`Tokens/ForgeTextRole.swift`): its system
    /// text style and weight, and the theme's text colour for its emphasis.
    ///
    ///     Text(book.title).forgeText(.rowTitle)
    ///     Text(book.author).forgeText(.metadata)
    ///     Text("12.4").forgeText(.figure)
    ///
    /// Dynamic Type, Bold Text and Increase Contrast apply as usual.
    public func forgeText(_ role: ForgeTextRole) -> some View {
        modifier(ForgeTextModifier(role: role))
    }
}

extension Font {
    /// The font of a ForgeUI text role, for places that take a `Font`
    /// rather than a view (an `AttributedString`, a `Label`'s title).
    public static func forge(_ role: ForgeTextRole) -> Font {
        var font = Font.system(role.style.swiftUI).weight(role.weight.swiftUI)
        if role.monospacedDigits {
            font = font.monospacedDigit()
        }
        return font
    }
}

private struct ForgeTextModifier: ViewModifier {
    let role: ForgeTextRole
    @Environment(\.forgeTheme) private var theme

    func body(content: Content) -> some View {
        content
            .font(.forge(role))
            .foregroundStyle(theme.colors.text(role.emphasis))
    }
}

extension ForgeTextStyle {
    var swiftUI: Font.TextStyle {
        switch self {
        case .largeTitle: .largeTitle
        case .title: .title
        case .title2: .title2
        case .title3: .title3
        case .headline: .headline
        case .subheadline: .subheadline
        case .body: .body
        case .callout: .callout
        case .footnote: .footnote
        case .caption: .caption
        case .caption2: .caption2
        }
    }
}

extension ForgeTextWeight {
    var swiftUI: Font.Weight {
        switch self {
        case .regular: .regular
        case .medium: .medium
        case .semibold: .semibold
        case .bold: .bold
        }
    }
}
#endif
