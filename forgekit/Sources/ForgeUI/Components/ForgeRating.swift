/// The rules `ForgeRating` draws, kept apart from SwiftUI so they are tested
/// on Linux: what a tap and a VoiceOver swipe do, and what VoiceOver says.
struct ForgeRatingState: Hashable, Sendable {
    /// The current rating, 1 to `maximum`, or nil when unrated.
    var value: Int?
    var maximum: Int
    /// Tapping the current star (or swiping down from 1) clears the rating.
    var allowsClearing: Bool

    init(value: Int?, maximum: Int = 5, allowsClearing: Bool = true) {
        self.maximum = max(1, maximum)
        // A value outside 1...maximum is drawn as unrated, never as more
        // stars than there are.
        if let value, (1...self.maximum).contains(value) {
            self.value = value
        } else {
            self.value = nil
        }
        self.allowsClearing = allowsClearing
    }

    /// Star `star` (1-based) is filled.
    func isFilled(_ star: Int) -> Bool { star <= (value ?? 0) }

    /// What tapping star `star` sets.
    func tapping(_ star: Int) -> Int? {
        if star == value { return allowsClearing ? nil : value }
        return min(max(star, 1), maximum)
    }

    /// What a VoiceOver swipe up sets.
    var incremented: Int? { min((value ?? 0) + 1, maximum) }

    /// What a VoiceOver swipe down sets: one fewer, and from 1 to unrated
    /// only when the rating can be cleared.
    var decremented: Int? {
        guard let value else { return nil }
        if value > 1 { return value - 1 }
        return allowsClearing ? nil : 1
    }

    /// VoiceOver's value: "3 of 5 stars", or "Not rated".
    var spokenValue: String {
        guard let value else { return "Not rated" }
        return "\(value) of \(maximum) stars"
    }
}

#if canImport(SwiftUI)
import SwiftUI

/// Stars to show or set a rating. The view model owns the value (an `Int?`,
/// nil when unrated); the rating only reports taps.
///
///     ForgeRating(value: book.rating, identifier: "bookDetail.rating") { model.rate(book, $0) }
///     ForgeRating(value: take.stars, identifier: "take.rating")   // read-only
///
/// - Tap a star to set it; tap the current star to clear it (unless
///   `allowsClearing` is false).
/// - VoiceOver hears one adjustable element: the label ("Rating" unless
///   you give one) and "3 of 5 stars" or "Not rated"; swipe up and down to
///   change it. Read-only ratings drop the adjustable action.
/// - Identifier: `<identifier>` on the one element. A UI test reads its
///   value ("3 of 5 stars"); the stars inside are not separate elements,
///   and `forge ui` has no adjust action yet, so a UI test can't set it.
/// - Stars take the warning tone's solid colour (a fixed warm colour that
///   `contrastIssues()` checks as a shape), never the accent, so ratings
///   look the same in every app. They scale with the surrounding font
///   (`.font(.title3)` for a rating that is the point of the screen), and
///   each star stays at least 44 pt to tap.
public struct ForgeRating: View {
    private let label: Text
    private let state: ForgeRatingState
    private let identifier: String
    private let onChange: ((Int?) -> Void)?

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled

    /// A rating the person can change. `onChange` gets the new value (nil
    /// when cleared), and is called only when the value changes.
    public init(
        _ label: LocalizedStringKey = "Rating",
        value: Int?,
        maximum: Int = 5,
        allowsClearing: Bool = true,
        identifier: String,
        onChange: @escaping (Int?) -> Void
    ) {
        self.label = Text(label)
        self.state = ForgeRatingState(value: value, maximum: maximum, allowsClearing: allowsClearing)
        self.identifier = identifier
        self.onChange = onChange
    }

    /// A rating to look at only.
    public init(
        _ label: LocalizedStringKey = "Rating",
        value: Int?,
        maximum: Int = 5,
        identifier: String
    ) {
        self.label = Text(label)
        self.state = ForgeRatingState(value: value, maximum: maximum)
        self.identifier = identifier
        self.onChange = nil
    }

    private var isInteractive: Bool { onChange != nil && isEnabled }

    public var body: some View {
        let filled = theme.colors.tone(.warning).solid
        HStack(spacing: onChange == nil ? theme.spacing.xxs : theme.spacing.xs) {
            ForEach(1...state.maximum, id: \.self) { star in
                let symbol = Image(systemName: state.isFilled(star) ? "star.fill" : "star")
                    .foregroundStyle(state.isFilled(star) ? AnyShapeStyle(filled) : AnyShapeStyle(theme.colors.textTertiary))
                    // An SF Symbol's name becomes its identifier; hide it so
                    // the rating is one element with one identifier.
                    .accessibilityHidden(true)
                if let onChange {
                    Button {
                        let new = state.tapping(star)
                        if new != state.value { onChange(new) }
                    } label: {
                        symbol.frame(minWidth: 44, minHeight: 44)
                    }
                    .buttonStyle(.borderless)
                } else {
                    symbol
                }
            }
        }
        .opacity(onChange != nil && !isEnabled ? 0.5 : 1)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityValue(Text(state.spokenValue))
        .accessibilityIdentifier(identifier)
        .modifier(ForgeRatingAdjustable(state: state, onChange: isInteractive ? onChange : nil))
    }
}

/// The adjustable action, only on a rating the person can change.
private struct ForgeRatingAdjustable: ViewModifier {
    let state: ForgeRatingState
    let onChange: ((Int?) -> Void)?

    func body(content: Content) -> some View {
        if let onChange {
            content.accessibilityAdjustableAction { direction in
                let new: Int?
                switch direction {
                case .increment: new = state.incremented
                case .decrement: new = state.decremented
                @unknown default: new = state.value
                }
                // Only a change is reported: a swipe past either end does nothing.
                if new != state.value { onChange(new) }
            }
        } else {
            content
        }
    }
}
#endif
