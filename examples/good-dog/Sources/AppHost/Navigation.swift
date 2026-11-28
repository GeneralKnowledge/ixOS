@preconcurrency import GoodDogFFI
import ForgeAppKit
import SwiftUI

/// Every screen a Good Dog stack can push: a goal's path, an exercise, a
/// glossary term, the glossary. One route type, so a term reached from an
/// exercise and again from a related term is the same screen, gone back to
/// rather than stacked twice (ForgeAppKit's Layered Navigation).
enum DogRoute: ForgeSurfaceRoute {
    case goal(UInt64)
    case exercise(ExerciseRoute)
    case term(String)
    case glossary

    var surfaceID: String? {
        switch self {
        case .goal(let id): "goal:\(id)"
        // The exercise, whichever activity it was opened from.
        case .exercise(let route): "exercise:\(route.exerciseId)"
        case .term(let id): "term:\(id)"
        case .glossary: "glossary"
        }
    }
}

/// A Good Dog navigation stack: its own path, every `DogRoute` destination,
/// and Layered Navigation on every push.
struct DogStack<Content: View>: View {
    let model: GoodDogModel
    @ViewBuilder let content: Content
    @State private var path: [DogRoute] = []

    var body: some View {
        NavigationStack(path: $path.forgeLayered()) {
            content.forgeLayeredDestination(for: DogRoute.self) { route in
                switch route {
                case .goal(let id): GoalPathView(model: model, goalId: id)
                case .exercise(let exercise): ExerciseView(model: model, route: exercise)
                case .term(let id): TermView(model: model, termId: id)
                case .glossary: GlossaryView(model: model)
                }
            }
        }
    }
}
