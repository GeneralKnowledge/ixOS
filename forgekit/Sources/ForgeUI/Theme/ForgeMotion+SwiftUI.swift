#if canImport(SwiftUI)
import SwiftUI

// The SwiftUI side of ForgeMotion. Every animation goes through here so
// that Reduce Motion turns it off in one place.

extension ForgeMotion {
    /// The SwiftUI animation for this kind of movement.
    public var animation: Animation {
        switch self {
        case .quick: .easeOut(duration: duration)
        case .standard: .easeInOut(duration: duration)
        case .emphasised: .spring(response: duration, dampingFraction: damping)
        case .ambient: .easeInOut(duration: duration)
        }
    }

    /// The animation, or none under Reduce Motion.
    public func animation(reduceMotion: Bool) -> Animation? {
        ForgeMotion.animates(reduceMotion: reduceMotion) ? animation : nil
    }
}

/// `withAnimation` for a ForgeMotion: the change happens at once under
/// Reduce Motion. Read `reduceMotion` from
/// `@Environment(\.accessibilityReduceMotion)`.
public func withForgeAnimation<Result>(
    _ motion: ForgeMotion,
    reduceMotion: Bool,
    _ body: () throws -> Result
) rethrows -> Result {
    try withAnimation(motion.animation(reduceMotion: reduceMotion), body)
}

extension View {
    /// `.animation(_:value:)` for a ForgeMotion, off under Reduce Motion.
    public func forgeAnimation<V: Equatable>(_ motion: ForgeMotion, value: V) -> some View {
        modifier(ForgeAnimationModifier(motion: motion, value: value))
    }
}

private struct ForgeAnimationModifier<V: Equatable>: ViewModifier {
    let motion: ForgeMotion
    let value: V
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.animation(motion.animation(reduceMotion: reduceMotion), value: value)
    }
}
#endif
