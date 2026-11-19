#if canImport(SwiftUI)
import SwiftUI

/// The top of a detail screen: an icon tile, a title, a subtitle, and an
/// optional row of badges or statuses under them.
///
///     ForgeDetailHeader(task.title, subtitle: "Due tomorrow", systemImage: "checklist") {
///         ForgeBadge("High", tone: .danger)
///     }
///
/// In a `List`, put it in a section of its own with
/// `.listRowBackground(Color.clear)`.
public struct ForgeDetailHeader<Accessory: View>: View {
    private let title: Text
    private let subtitle: Text?
    private let systemImage: String?
    private let tone: ForgeTone
    private let accessory: Accessory

    @Environment(\.forgeTheme) private var theme
    @ScaledMetric(relativeTo: .largeTitle) private var tileSize: CGFloat = 64

    public init(
        _ title: LocalizedStringKey,
        subtitle: LocalizedStringKey? = nil,
        systemImage: String? = nil,
        tone: ForgeTone = .accent,
        @ViewBuilder accessory: () -> Accessory
    ) {
        self.title = Text(title)
        self.subtitle = subtitle.map { Text($0) }
        self.systemImage = systemImage
        self.tone = tone
        self.accessory = accessory()
    }

    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ title: S,
        subtitle: String? = nil,
        systemImage: String? = nil,
        tone: ForgeTone = .accent,
        @ViewBuilder accessory: () -> Accessory
    ) {
        self.title = Text(title)
        self.subtitle = subtitle.map { Text($0) }
        self.systemImage = systemImage
        self.tone = tone
        self.accessory = accessory()
    }

    public var body: some View {
        VStack(spacing: theme.spacing.m) {
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: tileSize * 0.45, weight: .semibold))
                    .foregroundStyle(theme.colors.tone(tone).onSolid)
                    .frame(width: tileSize, height: tileSize)
                    .background(theme.colors.tone(tone).solid, in: RoundedRectangle(cornerRadius: theme.radius.l, style: .continuous))
                    .accessibilityHidden(true)
            }
            VStack(spacing: theme.spacing.xs) {
                title
                    .font(.title2.weight(.bold))
                    .foregroundStyle(theme.colors.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                if let subtitle {
                    subtitle
                        .font(.subheadline)
                        .foregroundStyle(theme.colors.textSecondary)
                }
            }
            .multilineTextAlignment(.center)
            HStack(spacing: theme.spacing.s) {
                accessory
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, theme.spacing.m)
    }
}

extension ForgeDetailHeader where Accessory == EmptyView {
    public init(_ title: LocalizedStringKey, subtitle: LocalizedStringKey? = nil, systemImage: String? = nil, tone: ForgeTone = .accent) {
        self.init(title, subtitle: subtitle, systemImage: systemImage, tone: tone) { EmptyView() }
    }

    @_disfavoredOverload
    public init<S: StringProtocol>(_ title: S, subtitle: String? = nil, systemImage: String? = nil, tone: ForgeTone = .accent) {
        self.init(title, subtitle: subtitle, systemImage: systemImage, tone: tone) { EmptyView() }
    }
}
#endif
