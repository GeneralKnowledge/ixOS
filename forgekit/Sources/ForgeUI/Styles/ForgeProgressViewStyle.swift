/// The numbers ForgeUI's progress style draws with, kept apart from SwiftUI
/// so they are tested on Linux.
enum ForgeProgress {
    /// A fraction drawn in range: a value past the end (a file that grew, a
    /// count off by one) fills the bar, never overflows it.
    static func clamped(_ fraction: Double?) -> Double? {
        guard let fraction, fraction.isFinite else { return nil }
        return min(max(fraction, 0), 1)
    }
}

#if canImport(SwiftUI)
import SwiftUI

/// ForgeUI's look for SwiftUI's own `ProgressView`:
/// `.progressViewStyle(.forge)`, or `.forge(tone:)` when the colour means
/// something (a book's status).
///
///     ProgressView(value: done, total: total) {
///         Text("Receiving 12 MB")
///     } currentValueLabel: {
///         Text("4 MB of 12 MB")
///     }
///     .progressViewStyle(.forge)
///
/// - The label takes the `rowTitle` text role and the current value the
///   `metadata` role with equal-width digits, on one line above the bar
///   (the value wraps under the label at large text sizes).
/// - The bar is SwiftUI's linear bar in the tone's solid colour, so
///   VoiceOver hears the label, the current value and the percentage as
///   one element. Write the current value as "n of m" when there is a
///   count.
/// - A value past either end draws as empty or full. Without a value it is
///   an indeterminate bar.
/// - Over about ten seconds of work, show this rather than a spinner.
public struct ForgeProgressViewStyle: ProgressViewStyle {
    let tone: ForgeTone

    public func makeBody(configuration: Configuration) -> some View {
        ForgeProgressBody(configuration: configuration, tone: tone)
    }
}

extension ProgressViewStyle where Self == ForgeProgressViewStyle {
    public static var forge: ForgeProgressViewStyle { ForgeProgressViewStyle(tone: .accent) }
    public static func forge(tone: ForgeTone) -> ForgeProgressViewStyle { ForgeProgressViewStyle(tone: tone) }
}

private struct ForgeProgressBody: View {
    let configuration: ProgressViewStyleConfiguration
    let tone: ForgeTone

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            if configuration.label != nil || configuration.currentValueLabel != nil {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .firstTextBaseline, spacing: theme.spacing.s) {
                        label
                        Spacer(minLength: theme.spacing.s)
                        currentValue
                    }
                    VStack(alignment: .leading, spacing: theme.spacing.xxs) {
                        label
                        currentValue
                    }
                }
            }
            bar
        }
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var label: some View {
        if let label = configuration.label {
            label
                .font(.forge(.rowTitle))
                .foregroundStyle(theme.colors.textPrimary)
        }
    }

    @ViewBuilder
    private var currentValue: some View {
        if let value = configuration.currentValueLabel {
            value
                .font(.forge(.metadata))
                .monospacedDigit()
                .foregroundStyle(theme.colors.textSecondary)
        }
    }

    @ViewBuilder
    private var bar: some View {
        // The style is set again on the inner bar so it doesn't draw itself.
        if let fraction = ForgeProgress.clamped(configuration.fractionCompleted) {
            ProgressView(value: fraction)
                .progressViewStyle(.linear)
                .tint(theme.colors.tone(tone).solid)
        } else {
            ProgressView()
                .progressViewStyle(.linear)
                .tint(theme.colors.tone(tone).solid)
        }
    }
}
#endif
