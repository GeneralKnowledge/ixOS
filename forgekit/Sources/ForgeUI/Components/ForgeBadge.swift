#if canImport(SwiftUI)
import SwiftUI

/// A short label in a tinted capsule: a state, a category, a count.
///
///     ForgeBadge("Overdue", tone: .warning)
///     ForgeBadge("3 new", systemImage: "sparkles", tone: .info)
public struct ForgeBadge: View {
    private let text: Text
    private let systemImage: String?
    private let tone: ForgeTone

    @Environment(\.forgeTheme) private var theme

    public init(_ text: LocalizedStringKey, systemImage: String? = nil, tone: ForgeTone = .neutral) {
        self.text = Text(text)
        self.systemImage = systemImage
        self.tone = tone
    }

    @_disfavoredOverload
    public init<S: StringProtocol>(_ text: S, systemImage: String? = nil, tone: ForgeTone = .neutral) {
        self.text = Text(text)
        self.systemImage = systemImage
        self.tone = tone
    }

    public var body: some View {
        let colors = theme.colors.tone(tone)
        HStack(spacing: theme.spacing.xs) {
            if let systemImage {
                Image(systemName: systemImage)
                    .accessibilityHidden(true)
            }
            text
        }
        .font(.caption.weight(.semibold))
        .lineLimit(1)
        .foregroundStyle(colors.text)
        .padding(.horizontal, theme.spacing.s)
        .padding(.vertical, theme.spacing.xxs)
        .background(colors.soft, in: Capsule())
        .accessibilityElement(children: .combine)
    }
}

/// A coloured dot and a label: "Synced", "Offline", "Needs attention".
///
///     ForgeStatus("Synced", tone: .success)
public struct ForgeStatus: View {
    private let text: Text
    private let tone: ForgeTone

    @Environment(\.forgeTheme) private var theme
    @ScaledMetric(relativeTo: .footnote) private var dot: CGFloat = 8

    public init(_ text: LocalizedStringKey, tone: ForgeTone) {
        self.text = Text(text)
        self.tone = tone
    }

    @_disfavoredOverload
    public init<S: StringProtocol>(_ text: S, tone: ForgeTone) {
        self.text = Text(text)
        self.tone = tone
    }

    public var body: some View {
        HStack(spacing: theme.spacing.s) {
            Circle()
                .fill(theme.colors.tone(tone).solid)
                .frame(width: dot, height: dot)
                .accessibilityHidden(true)
            text
                .foregroundStyle(theme.colors.textSecondary)
        }
        .font(.footnote.weight(.medium))
        .accessibilityElement(children: .combine)
    }
}

/// A small uppercase heading over a group of content outside a `List`
/// (inside one, use `Section("Account")`, which already looks like this),
/// with an optional trailing action such as "See All".
public struct ForgeSectionHeader: View {
    private let title: Text
    private let actionTitle: Text?
    private let action: (() -> Void)?

    @Environment(\.forgeTheme) private var theme

    public init(_ title: LocalizedStringKey) {
        self.title = Text(title)
        self.actionTitle = nil
        self.action = nil
    }

    /// Give the action button an identifier: `.accessibilityIdentifier` on
    /// the header applies to it.
    public init(_ title: LocalizedStringKey, actionTitle: LocalizedStringKey, action: @escaping () -> Void) {
        self.title = Text(title)
        self.actionTitle = Text(actionTitle)
        self.action = action
    }

    public var body: some View {
        HStack(alignment: .firstTextBaseline) {
            title
                .font(.footnote.weight(.semibold))
                .textCase(.uppercase)
                .foregroundStyle(theme.colors.textSecondary)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: theme.spacing.s)
            if let actionTitle, let action {
                Button(action: action) { actionTitle }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(theme.colors.accent.text)
            }
        }
        .padding(.horizontal, theme.spacing.xs)
    }
}
#endif
