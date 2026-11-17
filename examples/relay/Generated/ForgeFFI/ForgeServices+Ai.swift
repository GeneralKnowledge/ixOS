// Forge native services: the on-device language model (optional:
// `native_services!(ai)`), Apple Intelligence through Foundation Models on
// iOS 26. Source of truth: crates/forge-services/swift/optional/. Copied into
// an app's Generated/ForgeFFI only when its Rust crate selects the service.
//
// One adapter for every app (docs/forge-ai-plan.md, A0): it replaces Chat's
// FoundationModelsEngine and Pocket Library's AppleIntelligenceSummarizer,
// with what each of them learned on the phone.

import Foundation

#if canImport(FoundationModels)
    import FoundationModels
#endif

extension ForgeServices {
    static func installAi() throws {
        installNativeAi(model: ForgeAppleModel())
    }
}

/// `AiModel` over Apple's on-device model (Foundation Models, iOS 26).
///
/// Rust owns the request (instructions, history, trimming, when to stop);
/// this adapter turns it into text and pushes only the new part of each
/// snapshot into Rust's `TextSink`. Each answer gets a fresh session built
/// from a transcript: the system messages as instructions, each earlier
/// message as a real prompt or response entry, and the last message as the
/// prompt (a flattened "User: … You: …" script made the model carry on
/// writing both sides; Chat L1 phone run, 2026-11-08). Plain text only, no
/// `@Generable` (its macro needs a compiler plugin that only ships with
/// Xcode).
public final class ForgeAppleModel: AiModel, Sendable {
    public static let modelId = "apple-foundation-models"
    /// Apple's published context window for the on-device model.
    public static let contextTokens: UInt32 = 4096

    /// Apple's guardrails let the model work on text it is given (a book's
    /// chapter, an article) that its default guardrails decline: use it to
    /// transform the user's own content, not for open conversation.
    let permissiveContentTransformations: Bool

    public init(permissiveContentTransformations: Bool = false) {
        self.permissiveContentTransformations = permissiveContentTransformations
    }

    public func describe() -> AiModelInfo {
        let (available, reason) = Self.availability()
        return AiModelInfo(
            id: Self.modelId, name: "Apple Intelligence", available: available,
            unavailableReason: reason, contextTokens: Self.contextTokens)
    }

    public func generate(request: AiRequest, sink: TextSink) async throws {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                try await stream(request, into: sink)
                return
            }
        #endif
        throw AiModelError.Unavailable(reason: Self.availability().1 ?? "Needs iOS 26.")
    }

    /// Whether Apple's model can answer now, and why not in words the user
    /// can act on.
    public static func availability() -> (Bool, String?) {
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
        return (false, "The on-device model needs iOS 26 with Apple Intelligence.")
    }

    #if canImport(FoundationModels)
        @available(iOS 26.0, macOS 26.0, *)
        typealias FMTranscript = FoundationModels.Transcript

        /// The instructions and the earlier messages as the session's own
        /// history, so the model sees real turns and answers only the next.
        @available(iOS 26.0, macOS 26.0, *)
        static func transcript(_ instructions: String, _ earlier: [AiMessage]) -> FMTranscript {
            func segments(_ text: String) -> [FMTranscript.Segment] {
                [.text(FMTranscript.TextSegment(content: text))]
            }
            var entries: [FMTranscript.Entry] = []
            if !instructions.isEmpty {
                entries.append(
                    .instructions(
                        FMTranscript.Instructions(
                            segments: segments(instructions), toolDefinitions: [])))
            }
            for message in earlier {
                switch message.role {
                case .user:
                    entries.append(.prompt(FMTranscript.Prompt(segments: segments(message.content))))
                case .assistant:
                    entries.append(
                        .response(
                            FMTranscript.Response(assetIDs: [], segments: segments(message.content))))
                case .system:
                    break
                }
            }
            return FMTranscript(entries: entries)
        }

        /// The stream yields the whole answer so far each time; only the new
        /// part goes to Rust. Leaving the loop drops the stream, which ends
        /// the generation.
        @available(iOS 26.0, macOS 26.0, *)
        func stream(_ request: AiRequest, into sink: TextSink) async throws {
            let instructions = request.messages
                .filter { $0.role == .system }
                .map(\.content)
                .joined(separator: "\n\n")
            let conversation = request.messages.filter { $0.role != .system }
            let newest = conversation.last?.content ?? ""
            let model =
                permissiveContentTransformations
                ? SystemLanguageModel(guardrails: .permissiveContentTransformations)
                : SystemLanguageModel.default
            let session = LanguageModelSession(
                model: model,
                transcript: Self.transcript(instructions, Array(conversation.dropLast())))
            let options = GenerationOptions(
                temperature: request.temperature, maximumResponseTokens: Int(request.maxTokens))
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
                case .guardrailViolation, .refusal:
                    throw AiModelError.Refused(reason: "Apple Intelligence declined to answer this.")
                case .exceededContextWindowSize:
                    throw AiModelError.ContextTooLong(
                        reason: "This is too long for Apple Intelligence.")
                case .rateLimited:
                    throw AiModelError.Busy(
                        reason: "Apple Intelligence is limiting requests from the background.")
                case .assetsUnavailable:
                    throw AiModelError.Unavailable(reason: Self.availability().1 ?? "\(error)")
                default:
                    throw AiModelError.Engine(reason: "\(error)")
                }
            }
        }
    #endif
}
