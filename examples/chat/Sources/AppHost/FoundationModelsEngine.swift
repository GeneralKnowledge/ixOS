@preconcurrency import ForgeFFI
import Foundation

#if canImport(FoundationModels)
    import FoundationModels
#endif

/// The assistant's L1 engine: Apple's on-device model (Apple Intelligence)
/// through the Foundation Models framework, iOS 26 and later.
///
/// Rust owns the conversation (characters, history, trimming, when to stop);
/// this adapter only turns a `GenerateRequest` into text and pushes it into
/// Rust's `TokenSink` (docs/local-ai.md). Each answer gets a fresh session
/// built from a transcript: the character as instructions, then each earlier
/// message as a real prompt or response entry, and the newest message as the
/// prompt. (A flattened "User: … You: …" script made the model carry on
/// writing both sides; L1 phone run, 2026-11-08.) Plain text generation
/// only, no `@Generable` (its macro needs a compiler plugin that only ships
/// with Xcode).
public final class FoundationModelsEngine: LocalModelEngine, Sendable {
    public static let engineId = "apple-foundation-models"
    /// Apple's published context window for the on-device model.
    static let contextTokens: UInt32 = 4096

    public init() {}

    public func describe() -> EngineInfo {
        let (available, reason) = Self.availability()
        return EngineInfo(
            id: Self.engineId, name: "Apple Intelligence", available: available,
            unavailableReason: reason, contextTokens: Self.contextTokens)
    }

    public func generate(request: GenerateRequest, sink: TokenSink) async throws {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                try await Self.stream(request, into: sink)
                return
            }
        #endif
        throw AiError.Unavailable(reason: Self.availability().1 ?? "Needs iOS 26.")
    }

    static func availability() -> (Bool, String?) {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                switch SystemLanguageModel.default.availability {
                case .available:
                    return (true, nil)
                case .unavailable(.deviceNotEligible):
                    return (false, "This iPhone does not support Apple Intelligence.")
                case .unavailable(.appleIntelligenceNotEnabled):
                    return (
                        false,
                        "Turn on Apple Intelligence in Settings → Apple Intelligence & Siri."
                    )
                case .unavailable(.modelNotReady):
                    return (
                        false,
                        "Apple Intelligence is still downloading its model. Try again later."
                    )
                case .unavailable(let other):
                    return (false, "Apple Intelligence is unavailable (\(other)).")
                }
            }
        #endif
        return (false, "The on-device assistant needs iOS 26 with Apple Intelligence.")
    }

    #if canImport(FoundationModels)
        /// Apple's transcript type; the app has its own `Transcript` (the
        /// chat screen's rows), which would shadow it.
        @available(iOS 26.0, macOS 26.0, *)
        typealias FMTranscript = FoundationModels.Transcript

        /// The instructions and the earlier messages as the session's own
        /// history, so the model sees real turns and answers only the next.
        @available(iOS 26.0, macOS 26.0, *)
        static func transcript(_ instructions: String, _ earlier: [PromptTurn]) -> FMTranscript {
            func segments(_ text: String) -> [FMTranscript.Segment] {
                [.text(FMTranscript.TextSegment(content: text))]
            }
            var entries: [FMTranscript.Entry] = [
                .instructions(
                    FMTranscript.Instructions(segments: segments(instructions), toolDefinitions: []))
            ]
            for turn in earlier {
                switch turn.role {
                case .user:
                    entries.append(.prompt(FMTranscript.Prompt(segments: segments(turn.text))))
                case .assistant:
                    entries.append(
                        .response(FMTranscript.Response(assetIDs: [], segments: segments(turn.text))))
                }
            }
            return FMTranscript(entries: entries)
        }

        /// The stream yields the whole answer so far each time; only the new
        /// part goes to Rust. Leaving the loop drops the stream, which ends
        /// the generation.
        @available(iOS 26.0, macOS 26.0, *)
        static func stream(_ request: GenerateRequest, into sink: TokenSink) async throws {
            let options = GenerationOptions(
                temperature: request.temperature, maximumResponseTokens: Int(request.maxTokens))
            let newest = request.turns.last?.text ?? ""
            let session = LanguageModelSession(
                transcript: transcript(request.instructions, Array(request.turns.dropLast())))
            var sent = ""
            do {
                for try await snapshot in session.streamResponse(to: newest, options: options) {
                    let whole = snapshot.content
                    guard whole.count > sent.count, whole.hasPrefix(sent) else {
                        if sink.isStopped() || Task.isCancelled { return }
                        continue
                    }
                    let piece = String(whole.dropFirst(sent.count))
                    sent = whole
                    if !sink.push(text: piece) || Task.isCancelled { return }
                }
            } catch let error as LanguageModelSession.GenerationError {
                switch error {
                case .guardrailViolation:
                    throw AiError.Refused(
                        reason: "Apple Intelligence declined to answer this.")
                case .exceededContextWindowSize:
                    throw AiError.ContextTooLong(
                        reason: "The conversation is too long for Apple Intelligence.")
                default:
                    throw AiError.Engine(reason: "\(error)")
                }
            }
        }
    #endif
}
