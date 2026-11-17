@preconcurrency import ForgeFFI
import Foundation

#if canImport(FoundationModels)
    import FoundationModels
#endif

/// The AI Lab's structured-output experiment: Apple's guided generation
/// fills a typed value instead of writing free text.
///
/// This is also the A0 build test (docs/research/esim-ai-lab-plan.md):
/// `@Generable` is a Swift macro whose compiler plugin ships with Xcode, so
/// it may not build with xtool on Linux. If it doesn't, the fallback is
/// `DynamicGenerationSchema`, the same guided generation with the shape
/// described at run time.
enum AiLabStructured {
    /// The run for `AiLabModel`, or `nil` where Foundation Models is missing
    /// (before iOS 26).
    static var run: AiLabModel.StructuredRun? {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                return { instructions, prompt, update in
                    try await recipe(instructions: instructions, prompt: prompt, update: update)
                }
            }
        #endif
        return nil
    }
}

#if canImport(FoundationModels)
    /// What the structured experiment asks for.
    @available(iOS 26.0, macOS 26.0, *)
    @Generable
    struct LabRecipe {
        @Guide(description: "A short name for the dish")
        var name: String
        @Guide(description: "Minutes to make it", .range(1...240))
        var minutes: Int
        @Guide(description: "The ingredients, each a few words", .count(3...8))
        var ingredients: [String]
        @Guide(description: "The steps, each one sentence", .count(2...6))
        var steps: [String]
    }

    @available(iOS 26.0, macOS 26.0, *)
    extension LabRecipe {
        /// The typed value as the lab shows it: proof the fields came back
        /// as fields, not as text that looks like them.
        var summary: String {
            var lines = ["\(name) (\(minutes) min)"]
            lines.append("Ingredients: " + ingredients.joined(separator: ", "))
            for (index, step) in steps.enumerated() {
                lines.append("\(index + 1). \(step)")
            }
            return lines.joined(separator: "\n")
        }
    }

    extension AiLabStructured {
        @available(iOS 26.0, macOS 26.0, *)
        static func recipe(
            instructions: String, prompt: String,
            update: @escaping @Sendable (String) -> Void
        ) async throws -> String {
            guard case .available = SystemLanguageModel.default.availability else {
                throw AiModelError.Unavailable(
                    reason: ForgeAppleModel.availability().1 ?? "Apple Intelligence is unavailable.")
            }
            let session = LanguageModelSession(instructions: instructions)
            do {
                let stream = session.streamResponse(to: prompt, generating: LabRecipe.self)
                var last: GeneratedContent?
                for try await snapshot in stream {
                    try Task.checkCancellation()
                    // The partial value as JSON: fields fill in as they come.
                    update(snapshot.rawContent.jsonString)
                    last = snapshot.rawContent
                }
                guard let last else {
                    throw AiModelError.Engine(reason: "The model returned nothing.")
                }
                return try LabRecipe(last).summary
            } catch let error as LanguageModelSession.GenerationError {
                switch error {
                case .guardrailViolation, .refusal:
                    throw AiModelError.Refused(reason: "Apple Intelligence declined to answer this.")
                default:
                    throw AiModelError.Engine(reason: "\(error)")
                }
            }
        }
    }
#endif
