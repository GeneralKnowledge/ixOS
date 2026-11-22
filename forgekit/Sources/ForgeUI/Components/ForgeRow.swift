#if canImport(SwiftUI)
import SwiftUI

/// Where a row's icon tile takes its colour from.
enum ForgeRowTile {
    case tone(ForgeTone)
    case accent(ForgeAccent)

    func style(in theme: ForgeTheme) -> ForgeToneStyle {
        switch self {
        case .tone(let tone): theme.colors.tone(tone)
        case .accent(let accent): theme.colors.accent(accent)
        }
    }
}

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
/// Rows that stand for different things (Favourites, Playlists, Downloaded)
/// can each have their own colour from ForgeUI's checked accents, without
/// changing the app's accent: `ForgeRow("Favourites", systemImage:
/// "heart.fill", accent: .pink)`. Use a tone when the colour means
/// something (a warning, a danger); an accent when it only tells rows apart.
///
/// The row reads as one element in VoiceOver ("Notifications, Enabled").
public struct ForgeRow<Trailing: View>: View {
    private let title: Text
    private let subtitle: Text?
    private let systemImage: String?
    private let tile: ForgeRowTile
    private let trailing: Trailing

    @Environment(\.forgeTheme) private var theme
    @ScaledMetric(relativeTo: .body) private var tileSize: CGFloat = 30

    public var body: some View {
        HStack(spacing: theme.spacing.m) {
            if let systemImage {
                let colours = tile.style(in: theme)
                Image(systemName: systemImage)
                    .font(.system(size: tileSize * 0.55, weight: .semibold))
                    .foregroundStyle(colours.onSolid)
                    .frame(width: tileSize, height: tileSize)
                    .background(colours.solid, in: RoundedRectangle(cornerRadius: theme.radius.s, style: .continuous))
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
        self.tile = .tone(tone)
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
        self.tile = .tone(tone)
        self.trailing = trailing()
    }
}

extension ForgeRow {
    /// A row whose icon tile wears one of ForgeUI's accents, to tell it
    /// apart from the rows around it.
    public init(
        _ title: LocalizedStringKey,
        subtitle: LocalizedStringKey? = nil,
        systemImage: String,
        accent: ForgeAccent,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.title = Text(title)
        self.subtitle = subtitle.map { Text($0) }
        self.systemImage = systemImage
        self.tile = .accent(accent)
        self.trailing = trailing()
    }

    /// A row of app data whose icon tile wears one of ForgeUI's accents.
    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ title: S,
        subtitle: String? = nil,
        systemImage: String,
        accent: ForgeAccent,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.title = Text(title)
        self.subtitle = subtitle.map { Text($0) }
        self.systemImage = systemImage
        self.tile = .accent(accent)
        self.trailing = trailing()
    }
}

extension ForgeRow where Trailing == Text? {
    /// A row with an accent icon tile and an optional trailing value.
    public init(
        _ title: LocalizedStringKey,
        subtitle: LocalizedStringKey? = nil,
        systemImage: String,
        accent: ForgeAccent,
        value: LocalizedStringKey? = nil
    ) {
        self.init(title, subtitle: subtitle, systemImage: systemImage, accent: accent) {
            value.map { Text($0) }
        }
    }

    /// A row of app data with an accent icon tile and an optional trailing
    /// value, shown as written.
    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ title: S,
        subtitle: String? = nil,
        systemImage: String,
        accent: ForgeAccent,
        value: String? = nil
    ) {
        self.init(title, subtitle: subtitle, systemImage: systemImage, accent: accent) {
            value.map { Text($0) }
        }
    }

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
