#if canImport(SwiftUI)
import SwiftUI

/// A raised surface that groups related content outside a `List`:
///
///     ForgeCard {
///         Text("This week").font(.headline)
///         Text("4 of 6 tasks done")
///     }
///
/// Or style a `GroupBox` with `.groupBoxStyle(.forgeCard)`, which adds its
/// label as the card's heading.
public struct ForgeCard<Content: View>: View {
    private let content: Content

    @Environment(\.forgeTheme) private var theme
    @Environment(\.colorScheme) private var colorScheme

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        let shape = RoundedRectangle(cornerRadius: theme.radius.l, style: .continuous)
        let shadow = theme.cardShadow && colorScheme == .light
        VStack(alignment: .leading, spacing: theme.spacing.s) {
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(theme.spacing.l)
        .background(theme.colors.surfaceRaised, in: shape)
        .shadow(color: .black.opacity(shadow ? 0.08 : 0), radius: 12, x: 0, y: 4)
        .accessibilityElement(children: .contain)
    }
}

/// `GroupBox` drawn as a `ForgeCard`, with its label as the heading.
public struct ForgeCardGroupBoxStyle: GroupBoxStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        ForgeCard {
            configuration.label
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            configuration.content
        }
    }
}

extension GroupBoxStyle where Self == ForgeCardGroupBoxStyle {
    public static var forgeCard: ForgeCardGroupBoxStyle { ForgeCardGroupBoxStyle() }
}
#endif
