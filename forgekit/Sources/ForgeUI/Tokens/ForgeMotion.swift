// ForgeUI's motion vocabulary: four kinds, each with a duration, and the
// rule that Reduce Motion turns all of them off. Foundation only; the
// SwiftUI animations are in ForgeMotion+SwiftUI.swift.

/// A kind of movement. Pick by what moves, not by how it should feel.
public enum ForgeMotion: String, CaseIterable, Hashable, Sendable {
    /// Small state changes: a symbol, a badge. Ease-out, 0.2 s.
    case quick
    /// Insertions, removals, banners. Ease-in-out, 0.35 s.
    case standard
    /// Things a finger drives: artwork scale, a sheet settling. A spring
    /// (response 0.45, damping 0.8), so it can be interrupted.
    case emphasised
    /// Background colour and palette changes. Ease-in-out, 0.6 s.
    case ambient

    /// How long it runs, in seconds (a spring's response).
    public var duration: Double {
        switch self {
        case .quick: 0.2
        case .standard: 0.35
        case .emphasised: 0.45
        case .ambient: 0.6
        }
    }

    /// Whether it is a spring.
    public var isSpring: Bool { self == .emphasised }

    /// A spring's damping fraction (1 for the timed curves).
    public var damping: Double { isSpring ? 0.8 : 1 }

    /// Whether to animate at all: never under Reduce Motion.
    public static func animates(reduceMotion: Bool) -> Bool { !reduceMotion }
}
