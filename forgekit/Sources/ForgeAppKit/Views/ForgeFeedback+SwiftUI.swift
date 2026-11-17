#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Plays the model's feedback each time it asks for some
    /// (`ForgeFeedback.play(_:)`). Attach once per screen.
    ///
    ///     .forgeFeedback(model.feedback)
    public func forgeFeedback(_ feedback: ForgeFeedback) -> some View {
        sensoryFeedback(trigger: feedback) { _, new in
            new.kind?.sensoryFeedback
        }
    }
}

extension ForgeFeedback.Kind {
    /// The system feedback each kind plays.
    public var sensoryFeedback: SensoryFeedback {
        switch self {
        case .success: .success
        case .failure: .error
        case .selection: .selection
        case .refresh: .impact(weight: .light)
        }
    }
}
#endif
