// Named text roles: what a piece of text is (a row's title, its metadata, a
// figure), each defined once on top of a system text style, so Dynamic Type,
// Bold Text and new iOS styles still apply. Foundation only; the SwiftUI
// side is `.forgeText(_:)` (Theme/ForgeText+SwiftUI.swift).
//
// Use a role where the same kind of text appears on many screens, so every
// screen and every app agrees on it. Plain system styles (`.font(.body)`)
// stay fine for one-off text.

/// What a piece of text is: `Text(book.author).forgeText(.metadata)`.
public enum ForgeTextRole: String, CaseIterable, Identifiable, Hashable, Sendable {
    /// A big title inside content, where the navigation bar's title isn't
    /// used (a detail header, an onboarding page).
    case screenTitle
    /// A heading over a group of content that isn't a list section.
    case sectionTitle
    /// The main line of a row or card.
    case rowTitle
    /// Reading text.
    case body
    /// Facts about an item under or beside its title: an author, a date, a
    /// size, a count.
    case metadata
    /// A short explanation under a control or at the end of a section.
    case note
    /// A number that is the point of the screen or card: a temperature, a
    /// distance, a duration. Digits keep their width as they change.
    case figure
    /// A small number or word beside a value: a unit, a percentage, a time.
    case figureCaption

    public var id: String { rawValue }

    /// The system text style underneath, by its SwiftUI name.
    public var style: ForgeTextStyle {
        switch self {
        case .screenTitle: .title
        case .sectionTitle: .title3
        case .rowTitle: .headline
        case .body: .body
        case .metadata: .subheadline
        case .note: .footnote
        case .figure: .title
        case .figureCaption: .caption
        }
    }

    public var weight: ForgeTextWeight {
        switch self {
        case .screenTitle: .bold
        case .sectionTitle, .rowTitle, .figure: .semibold
        case .body, .metadata, .note: .regular
        case .figureCaption: .medium
        }
    }

    /// Which of the theme's text colours it is drawn in.
    public var emphasis: ForgeTextEmphasis {
        switch self {
        case .screenTitle, .sectionTitle, .rowTitle, .body, .figure: .primary
        case .metadata, .note, .figureCaption: .secondary
        }
    }

    /// Digits of equal width, so a changing number doesn't jitter.
    public var monospacedDigits: Bool {
        switch self {
        case .figure, .figureCaption: true
        default: false
        }
    }
}

/// The system text styles, by their SwiftUI names.
public enum ForgeTextStyle: String, CaseIterable, Hashable, Sendable {
    case largeTitle, title, title2, title3, headline, subheadline, body, callout, footnote, caption, caption2
}

public enum ForgeTextWeight: String, Hashable, Sendable {
    case regular, medium, semibold, bold
}

/// Primary text, or the quieter secondary and tertiary text colours.
public enum ForgeTextEmphasis: String, Hashable, Sendable {
    case primary, secondary, tertiary
}
