#if canImport(SwiftUI)
import SwiftUI

/// The row of a settings screen or a list: an optional icon tile, a title,
/// an optional subtitle, and an optional trailing value. Use it as the
/// content or label of SwiftUI's own rows:
///
///     NavigationLink { ProfileView() } label: {
///         ForgeRow("Your profile", subtitle: "Personal information", systemImage: "person.fill")
///     }
///     Toggle(isOn: $notifications) {
///         ForgeRow("Notifications", systemImage: "bell.badge.fill", tone: .danger)
///     }
///     ForgeRow("Version", value: "1.0.0")
///
/// The row reads as one element in VoiceOver ("Notifications, Enabled").
public struct ForgeRow<Trailing: View>: View {
    private let title: Text
    private let subtitle: Text?
    private let systemImage: String?
    private let tone: ForgeTone
    private let trailing: Trailing

    @Environment(\.forgeTheme) private var theme
    @ScaledMetric(relativeTo: .body) private var tileSize: CGFloat = 30

    public var body: some View {
        HStack(spacing: theme.spacing.m) {
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: tileSize * 0.55, weight: .semibold))
                    .foregroundStyle(theme.colors.tone(tone).onSolid)
                    .frame(width: tileSize, height: tileSize)
                    .background(theme.colors.tone(tone).solid, in: RoundedRectangle(cornerRadius: theme.radius.s, style: .continuous))
                    .accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: theme.spacing.xxs) {
                title
                    .font(.body)
                    .foregroundStyle(theme.colors.textPrimary)
                if let subtitle {
                    subtitle
                        .font(.subheadline)
                        .foregroundStyle(theme.colors.textSecondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing
                .foregroundStyle(theme.colors.textSecondary)
        }
        .padding(.vertical, theme.spacing.xxs)
        .accessibilityElement(children: .combine)
    }
}

extension ForgeRow {
    /// A row with any trailing view, for example a badge or a status.
    public init(
        _ title: LocalizedStringKey,
        subtitle: LocalizedStringKey? = nil,
        systemImage: String? = nil,
        tone: ForgeTone = .accent,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.title = Text(title)
        self.subtitle = subtitle.map { Text($0) }
        self.systemImage = systemImage
        self.tone = tone
        self.trailing = trailing()
    }

    /// A row of app data (a task's title, a book's author), shown as written.
    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ title: S,
        subtitle: String? = nil,
        systemImage: String? = nil,
        tone: ForgeTone = .accent,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.title = Text(title)
        self.subtitle = subtitle.map { Text($0) }
        self.systemImage = systemImage
        self.tone = tone
        self.trailing = trailing()
    }
}

extension ForgeRow where Trailing == Text? {
    /// A row with an optional trailing value ("Enabled", "1.0.0").
    public init(
        _ title: LocalizedStringKey,
        subtitle: LocalizedStringKey? = nil,
        systemImage: String? = nil,
        tone: ForgeTone = .accent,
        value: LocalizedStringKey? = nil
    ) {
        self.init(title, subtitle: subtitle, systemImage: systemImage, tone: tone) {
            value.map { Text($0) }
        }
    }

    /// A row of app data with an optional trailing value, shown as written.
    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ title: S,
        subtitle: String? = nil,
        systemImage: String? = nil,
        tone: ForgeTone = .accent,
        value: String? = nil
    ) {
        self.init(title, subtitle: subtitle, systemImage: systemImage, tone: tone) {
            value.map { Text($0) }
        }
    }
}
#endif
