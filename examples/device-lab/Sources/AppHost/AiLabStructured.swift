@preconcurrency import ForgeFFI
import Foundation

#if canImport(FoundationModels)
    import FoundationModels
#endif

/// The AI Lab's structured-output experiment: Apple's guided generation
/// fills a typed value instead of writing free text.
///
/// Two ways to ask for the same typed answer:
/// - `.structured`: Apple's `@Generable` macro (`LabRecipe`). Its compiler
///   plugin ships with Xcode, so this is the A0 build test of whether it
///   builds with xtool on Linux (docs/research/esim-ai-lab-plan.md).
/// - `.structuredSchema`: no macro (`LabMeal`, `StructuredShape`): the shape
///   is a plain value that becomes a `DynamicGenerationSchema` at run time,
///   and the answer decodes with `JSONDecoder`. Builds anywhere.
enum AiLabStructured {
    /// The run for `AiLabModel`, or `nil` where Foundation Models is missing
    /// (before iOS 26).
    static var run: AiLabModel.StructuredRun? {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                return { kind, instructions, prompt, update in
                    switch kind {
                    case .structuredSchema:
                        return try await meal(
                            instructions: instructions, prompt: prompt, update: update)
                    default:
                        return try await recipe(
                            instructions: instructions, prompt: prompt, update: update)
                    }
                }
            }
        #endif
        return nil
    }
}

/// The same answer as `LabRecipe`, without the macro: an ordinary
/// `Decodable` struct and its shape.
struct LabMeal: StructuredAnswer, Equatable {
    var name: String
    var minutes: Int
    var ingredients: [String]
    var steps: [String]

    static let shape = StructuredShape.object(
        name: "LabMeal",
        fields: [
            StructuredField("name", .string(description: "A short name for the dish")),
            StructuredField("minutes", .integer(description: "Minutes to make it", range: 1...240)),
            StructuredField(
                "ingredients",
                .array(of: .string(), description: "The ingredients, each a few words", count: 3...8)),
            StructuredField(
                "steps", .array(of: .string(), description: "The steps, each one sentence", count: 2...6)),
        ])

    var summary: String {
        var lines = ["\(name) (\(minutes) min)"]
        lines.append("Ingredients: " + ingredients.joined(separator: ", "))
        for (index, step) in steps.enumerated() {
            lines.append("\(index + 1). \(step)")
        }
        return lines.joined(separator: "\n")
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
                throw modelError(error)
            }
        }

        /// The same request through `StructuredShape`: no macro involved.
        @available(iOS 26.0, macOS 26.0, *)
        static func meal(
            instructions: String, prompt: String,
            update: @escaping @Sendable (String) -> Void
        ) async throws -> String {
            guard case .available = SystemLanguageModel.default.availability else {
                throw AiModelError.Unavailable(
                    reason: ForgeAppleModel.availability().1 ?? "Apple Intelligence is unavailable.")
            }
            let schema = try LabMeal.generationSchema()
            let session = LanguageModelSession(instructions: instructions)
            do {
                let stream = session.streamResponse(to: prompt, schema: schema)
                var last = ""
                for try await snapshot in stream {
                    try Task.checkCancellation()
                    last = snapshot.rawContent.jsonString
                    update(last)
                }
                return try LabMeal.decode(json: last).summary
            } catch let error as LanguageModelSession.GenerationError {
                throw modelError(error)
            }
        }

        @available(iOS 26.0, macOS 26.0, *)
        static func modelError(_ error: LanguageModelSession.GenerationError) -> AiModelError {
            switch error {
            case .guardrailViolation, .refusal:
                .Refused(reason: "Apple Intelligence declined to answer this.")
            default:
                .Engine(reason: "\(error)")
            }
        }
    }
#endif
