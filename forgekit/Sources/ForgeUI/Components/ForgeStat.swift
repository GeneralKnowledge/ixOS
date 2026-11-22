/// How a `ForgeStat` lays out its parts.
public enum ForgeStatLayout: Hashable, Sendable {
    /// The figure over its label, centred: a row of totals (distance, time,
    /// climb).
    case stacked
    /// The figure, then its unit over its label: a line in a card ("29",
    /// "KM/H" over "Wind").
    case inline
}

/// Whether a `ForgeStat` draws its own surface.
public enum ForgeStatBackground: Hashable, Sendable {
    /// A soft rounded surface, so a row of stats reads as tiles.
    case automatic
    /// No surface, and the text takes the colours around it, for an app
    /// with its own surfaces (Weather's sky cards).
    case hidden
}

/// What VoiceOver hears for a stat, kept apart from SwiftUI so it is
/// tested on Linux.
enum ForgeStatSpeech {
    /// The value as spoken: the caller's `spokenValue` ("4.2 kilometres"),
    /// else the value and unit as shown ("29 km/h").
    static func value(_ value: String, unit: String?, spokenValue: String?) -> String {
        if let spokenValue, !spokenValue.isEmpty { return spokenValue }
        guard let unit, !unit.isEmpty else { return value }
        return "\(value) \(unit)"
    }
}

#if canImport(SwiftUI)
import SwiftUI

/// One figure with its name: a total, a measurement, a count.
///
///     HStack {
///         ForgeStat("Distance", value: "4.2 km", spokenValue: "4.2 kilometres")
///         ForgeStat("Time", value: "1:12:05", spokenValue: "1 hour 12 minutes")
///     }
///     ForgeStat("Wind", value: "29", unit: "km/h", layout: .inline)
///         .forgeStatBackground(.hidden)
///
/// - The figure uses the `figure` text role (equal-width digits, so a
///   changing number doesn't jitter) and the label the `figureCaption` role.
/// - VoiceOver hears one element: "Distance, 4.2 kilometres". Give a
///   `spokenValue` when the shown value abbreviates ("km", "1:12:05").
/// - A `tone` colours the figure when the number means something (over a
///   limit); otherwise leave it out.
/// - A stacked stat fills the width it is offered, so stats in an `HStack`
///   share it equally.
public struct ForgeStat: View {
    private let label: Text
    private let value: String
    private let unit: String?
    private let spokenValue: String?
    private let tone: ForgeTone?
    private let layout: ForgeStatLayout

    @Environment(\.forgeTheme) private var theme
    @Environment(\.forgeStatBackground) private var background

    public init(
        _ label: LocalizedStringKey,
        value: String,
        unit: String? = nil,
        spokenValue: String? = nil,
        tone: ForgeTone? = nil,
        layout: ForgeStatLayout = .stacked
    ) {
        self.label = Text(label)
        self.value = value
        self.unit = unit
        self.spokenValue = spokenValue
        self.tone = tone
        self.layout = layout
    }

    /// A stat named by the app's data, shown as written.
    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ label: S,
        value: String,
        unit: String? = nil,
        spokenValue: String? = nil,
        tone: ForgeTone? = nil,
        layout: ForgeStatLayout = .stacked
    ) {
        self.label = Text(label)
        self.value = value
        self.unit = unit
        self.spokenValue = spokenValue
        self.tone = tone
        self.layout = layout
    }

    private var ownSurface: Bool { background != .hidden }

    private var figureStyle: AnyShapeStyle {
        if let tone { return AnyShapeStyle(theme.colors.tone(tone).text) }
        return ownSurface ? AnyShapeStyle(theme.colors.textPrimary) : AnyShapeStyle(.primary)
    }

    private var captionStyle: AnyShapeStyle {
        ownSurface ? AnyShapeStyle(theme.colors.textSecondary) : AnyShapeStyle(.secondary)
    }

    public var body: some View {
        content
            .padding(ownSurface ? theme.spacing.s : 0)
            .frame(maxWidth: layout == .stacked ? .infinity : nil)
            .background {
                if ownSurface {
                    RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
                        .fill(.fill.tertiary)
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(label)
            .accessibilityValue(Text(ForgeStatSpeech.value(value, unit: unit, spokenValue: spokenValue)))
    }

    @ViewBuilder
    private var content: some View {
        switch layout {
        case .stacked:
            VStack(spacing: theme.spacing.xxs) {
                HStack(alignment: .firstTextBaseline, spacing: theme.spacing.xxs) {
                    Text(value)
                        .font(.forge(.figure))
                        .monospacedDigit()
                        .foregroundStyle(figureStyle)
                    if let unit, !unit.isEmpty {
                        Text(unit)
                            .font(.forge(.figureCaption))
                            .foregroundStyle(captionStyle)
                    }
                }
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                label
                    .font(.forge(.figureCaption))
                    .foregroundStyle(captionStyle)
                    .multilineTextAlignment(.center)
            }
        case .inline:
            HStack(alignment: .center, spacing: theme.spacing.s) {
                Text(value)
                    .font(.forge(.figure))
                    .monospacedDigit()
                    .foregroundStyle(figureStyle)
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
                VStack(alignment: .leading, spacing: 0) {
                    if let unit, !unit.isEmpty {
                        Text(unit.uppercased())
                            .font(.forge(.figureCaption))
                            .foregroundStyle(captionStyle)
                    }
                    label
                        .font(.forge(.figureCaption))
                        .foregroundStyle(captionStyle)
                }
                Spacer(minLength: 0)
            }
        }
    }
}

private struct ForgeStatBackgroundKey: EnvironmentKey {
    static let defaultValue = ForgeStatBackground.automatic
}

extension EnvironmentValues {
    public var forgeStatBackground: ForgeStatBackground {
        get { self[ForgeStatBackgroundKey.self] }
        set { self[ForgeStatBackgroundKey.self] = newValue }
    }
}

extension View {
    /// `.hidden` drops the stats' own surface inside this view, and their
    /// text takes the colours around it.
    public func forgeStatBackground(_ background: ForgeStatBackground) -> some View {
        environment(\.forgeStatBackground, background)
    }
}
#endif
