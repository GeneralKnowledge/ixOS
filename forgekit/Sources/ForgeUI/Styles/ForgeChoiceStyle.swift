/// The shape of a ForgeUI choice.
public enum ForgeChoiceShape: Hashable, Sendable {
    /// A capsule around a short label: filters, reactions, one of a few
    /// options in a row.
    case chip
    /// A rounded tile around larger content: side A or B, a result to pick.
    case tile
    /// A circle around a symbol or a letter.
    case circle
}

/// What a choice looks like, kept apart from SwiftUI so it is tested on
/// Linux.
struct ForgeChoiceLook: Hashable, Sendable {
    var isSelected: Bool
    var shape: ForgeChoiceShape

    /// A chosen choice is filled with its tone's soft colour and outlined
    /// in its solid colour; an unchosen one sits on a neutral fill. Both
    /// are told apart by more than colour: the outline, the weight of the
    /// label, and (for chips) a checkmark.
    var usesToneFill: Bool { isSelected }
    var showsOutline: Bool { isSelected }
    var isBold: Bool { isSelected }
    var showsCheckmark: Bool { isSelected && shape == .chip }

    /// Every choice is at least 44 points to tap, however small it looks.
    static let minimumTapSize: Double = 44
}

#if canImport(SwiftUI)
import SwiftUI

/// One of several: `.buttonStyle(.forgeChoice(isSelected: side == .a))`.
/// The model owns which is chosen; the button only reports the tap. Adds
/// the `.isSelected` trait, so VoiceOver says "Selected".
///
///     ForEach(sides) { side in
///         Button(side.title) { model.choose(side) }
///             .buttonStyle(.forgeChoice(isSelected: model.side == side, shape: .tile))
///             .accessibilityIdentifier("compare.\(side.id)")
///     }
public struct ForgeChoiceButtonStyle: ButtonStyle {
    let isSelected: Bool
    let shape: ForgeChoiceShape
    let tone: ForgeTone

    public func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .forgeChoiceLook(isSelected: isSelected, shape: shape, tone: tone, isPressed: configuration.isPressed)
    }
}

extension ButtonStyle where Self == ForgeChoiceButtonStyle {
    /// A choice among several. `tone` is the accent unless the choice means
    /// something (a passed or failed check).
    public static func forgeChoice(
        isSelected: Bool, shape: ForgeChoiceShape = .chip, tone: ForgeTone = .accent
    ) -> ForgeChoiceButtonStyle {
        ForgeChoiceButtonStyle(isSelected: isSelected, shape: shape, tone: tone)
    }
}

/// A chip that turns on and off: `Toggle("Unread", isOn: $unreadOnly)
/// .toggleStyle(.forgeChoice)`. Several can be on at once (filters,
/// reactions). VoiceOver hears a button that says "Selected" when on.
public struct ForgeChoiceToggleStyle: ToggleStyle {
    let tone: ForgeTone

    public func makeBody(configuration: Configuration) -> some View {
        Button {
            configuration.isOn.toggle()
        } label: {
            configuration.label
        }
        .buttonStyle(ForgeChoiceButtonStyle(isSelected: configuration.isOn, shape: .chip, tone: tone))
    }
}

extension ToggleStyle where Self == ForgeChoiceToggleStyle {
    public static var forgeChoice: ForgeChoiceToggleStyle { ForgeChoiceToggleStyle(tone: .accent) }
    public static func forgeChoice(tone: ForgeTone) -> ForgeChoiceToggleStyle { ForgeChoiceToggleStyle(tone: tone) }
}

extension View {
    /// ForgeUI's choice look on any view (`ForgeChoiceButtonStyle` draws
    /// with it), with the `.isSelected` trait.
    public func forgeChoiceLook(
        isSelected: Bool, shape: ForgeChoiceShape = .chip, tone: ForgeTone = .accent, isPressed: Bool = false
    ) -> some View {
        ForgeChoiceChrome(
            label: self, look: ForgeChoiceLook(isSelected: isSelected, shape: shape), tone: tone, isPressed: isPressed)
    }
}

private struct ForgeChoiceChrome<Label: View>: View {
    let label: Label
    let look: ForgeChoiceLook
    let tone: ForgeTone
    let isPressed: Bool

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.controlSize) private var controlSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let colours = theme.colors.tone(tone)
        let compact = controlSize == .small || controlSize == .mini
        let minimum = CGFloat(ForgeChoiceLook.minimumTapSize)
        HStack(spacing: theme.spacing.xs) {
            if look.showsCheckmark {
                Image(systemName: "checkmark")
                    .font(.caption.weight(.bold))
                    .accessibilityHidden(true)
            }
            label
        }
        .fontWeight(look.isBold ? .semibold : .regular)
        .foregroundStyle(look.isSelected ? AnyShapeStyle(colours.text) : AnyShapeStyle(theme.colors.textPrimary))
        .padding(.horizontal, look.shape == .chip ? (compact ? theme.spacing.s : theme.spacing.m) : theme.spacing.s)
        .padding(.vertical, look.shape == .chip ? (compact ? theme.spacing.xxs : theme.spacing.xs) : theme.spacing.s)
        .frame(
            minWidth: look.shape == .circle ? minimum : nil,
            minHeight: look.shape == .chip && compact ? nil : minimum
        )
        .background { background(colours) }
        // A compact chip looks small but still takes a 44-point tap.
        .frame(minWidth: minimum, minHeight: minimum)
        .contentShape(Rectangle())
        .opacity(isEnabled ? 1 : 0.5)
        .scaleEffect(isPressed && !reduceMotion ? 0.96 : 1)
        .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: isPressed)
        .accessibilityAddTraits(look.isSelected ? .isSelected : [])
    }

    @ViewBuilder
    private func background(_ colours: ForgeToneStyle) -> some View {
        let fill = look.usesToneFill ? AnyShapeStyle(colours.soft) : AnyShapeStyle(.fill.secondary)
        let outline = look.showsOutline ? AnyShapeStyle(colours.solid) : AnyShapeStyle(Color.clear)
        switch look.shape {
        case .chip:
            Capsule().fill(fill).overlay { Capsule().strokeBorder(outline, lineWidth: 1.5) }
        case .circle:
            Circle().fill(fill).overlay { Circle().strokeBorder(outline, lineWidth: 2) }
        case .tile:
            let shape = RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
            shape.fill(fill).overlay { shape.strokeBorder(outline, lineWidth: 2) }
        }
    }
}
#endif
