@preconcurrency import PocketLibraryFFI
import Foundation

#if canImport(FoundationModels)
    import FoundationModels
#endif

/// Chapter summaries' model: Apple's on-device model (Apple Intelligence)
/// through the Foundation Models framework, iOS 26 and later.
///
/// Rust plans the summary (rust/src/summary.rs): it splits a long chapter
/// into parts that fit the model's 4,096-token window, and asks for notes
/// on each part, then a summary of the notes. This adapter answers one
/// request at a time, in a fresh session, and passes the answer so far to
/// Rust's sink, which shows the final summary as it is written and says
/// when to stop.
///
/// The model uses Apple's guardrails for transforming text the person
/// supplied (`permissiveContentTransformations`): a novel's violence or
/// grief would otherwise often be refused. Plain text generation only, no
/// `@Generable` (its macro needs a compiler plugin that only ships with
/// Xcode).
public final class AppleIntelligenceSummarizer: SummaryModel, Sendable {
    public init() {}

    public func describe() -> SummaryModelInfo {
        let (available, reason) = Self.availability()
        return SummaryModelInfo(name: "Apple Intelligence", available: available, unavailableReason: reason)
    }

    public func generate(request: SummaryRequest, sink: SummarySink) async throws -> String {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                return try await Self.answer(request, sink: sink)
            }
        #endif
        throw SummaryModelError.Unavailable(reason: Self.availability().1 ?? "Chapter summaries need iOS 26.")
    }

    static func availability() -> (Bool, String?) {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                switch SystemLanguageModel.default.availability {
                case .available:
                    return (true, nil)
                case .unavailable(.deviceNotEligible):
                    return (false, "This iPhone doesn't support Apple Intelligence.")
                case .unavailable(.appleIntelligenceNotEnabled):
                    return (false, "Turn on Apple Intelligence in Settings → Apple Intelligence & Siri.")
                case .unavailable(.modelNotReady):
                    return (false, "Apple Intelligence is still downloading its model. Try again later.")
                case .unavailable(let other):
                    return (false, "Apple Intelligence is unavailable (\(other)).")
                }
            }
        #endif
        return (false, "Chapter summaries need iOS 26 with Apple Intelligence.")
    }

    #if canImport(FoundationModels)
        /// The stream yields the whole answer so far each time. Leaving the
        /// loop drops the stream, which ends the generation.
        @available(iOS 26.0, macOS 26.0, *)
        static func answer(_ request: SummaryRequest, sink: SummarySink) async throws -> String {
            let model = SystemLanguageModel(guardrails: .permissiveContentTransformations)
            let session = LanguageModelSession(model: model, instructions: request.instructions)
            let options = GenerationOptions(
                temperature: request.temperature, maximumResponseTokens: Int(request.maxTokens))
            var answer = ""
            do {
                for try await snapshot in session.streamResponse(to: request.prompt, options: options) {
                    answer = snapshot.content
                    if !sink.update(textSoFar: answer) || Task.isCancelled {
                        throw SummaryModelError.Stopped
                    }
                }
            } catch let error as LanguageModelSession.GenerationError {
                switch error {
                case .guardrailViolation, .refusal:
                    throw SummaryModelError.Refused(reason: "Apple Intelligence declined this passage.")
                case .exceededContextWindowSize:
                    throw SummaryModelError.ContextTooLong(reason: "\(error)")
                case .assetsUnavailable:
                    throw SummaryModelError.Unavailable(reason: availability().1 ?? "\(error)")
                default:
                    throw SummaryModelError.Engine(reason: "\(error)")
                }
            }
            return answer
        }
    #endif
}
