@preconcurrency import ForgeFFI
import Foundation
import llama

#if canImport(os)
    import os
#endif

/// GGUF models on the phone through llama.cpp's prebuilt XCFramework (Metal
/// kernels embedded, so nothing compiles on Linux). Built for PrismML's
/// Ternary-Bonsai-2 models in Q2_0, the 1060 lab's low-bit finding: the 4B
/// scored 44/50 on GSM8K against Qwen3.5-4B's 46 at a third of the size.
///
/// An experiment in Device Lab's AI Lab, not a framework service yet
/// (docs/local-ai.md names llama.cpp as the second engine; it moves into
/// forge-services only once it has proven itself on the phone).

/// A model AI Lab can download and run.
struct LlamaModelFile: Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let fileName: String
    /// Hugging Face download, pinned to a revision.
    let url: URL
    /// Approximate size, for the free-space check and the progress bar.
    let bytes: Int64
    let note: String

    static let bonsai4B = LlamaModelFile(
        id: "bonsai-2-4b-q2_0",
        name: "Ternary-Bonsai-2 4B",
        fileName: "Ternary-Bonsai-2-4B-Q2_0.gguf",
        url: URL(
            string:
                "https://huggingface.co/prism-ml/Ternary-Bonsai-2-4B-gguf/resolve/50ebba61be85e5be07e8083db468ace0fc1bac43/Ternary-Bonsai-2-4B-Q2_0.gguf"
        )!,
        bytes: 1_210_000_000,
        note: "1.2 GB. Close to Qwen3.5 4B on maths in the 1060 lab.")

    static let bonsai9B = LlamaModelFile(
        id: "bonsai-2-9b-q2_0",
        name: "Ternary-Bonsai-2 9B",
        fileName: "Ternary-Bonsai-2-9B-Q2_0.gguf",
        url: URL(
            string:
                "https://huggingface.co/prism-ml/Ternary-Bonsai-2-9B-gguf/resolve/main/Ternary-Bonsai-2-9B-Q2_0.gguf"
        )!,
        bytes: 2_560_000_000,
        note: "2.6 GB. A memory test: it may not fit in what iOS gives the app.")

    static let catalogue: [LlamaModelFile] = [.bonsai4B, .bonsai9B]

    /// Application Support/Models/llama, kept out of iCloud backups.
    static var folder: URL {
        get throws {
            let base = try FileManager.default.url(
                for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil,
                create: true)
            var folder = base.appendingPathComponent("Models/llama", isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try folder.setResourceValues(values)
            return folder
        }
    }

    var localURL: URL? { try? Self.folder.appendingPathComponent(fileName) }

    var isDownloaded: Bool {
        guard let path = localURL?.path else { return false }
        return FileManager.default.fileExists(atPath: path)
    }
}

/// AI Lab's model interface over one GGUF file. Rust calls `generate` from
/// its own thread, one request at a time; the weights load on the first
/// answer and stay loaded (one model at a time, see `LlamaRuntime`).
final class LlamaCppModel: AiModel, @unchecked Sendable {
    static let contextTokens: UInt32 = 4096

    let file: LlamaModelFile

    init(file: LlamaModelFile) {
        self.file = file
    }

    func describe() -> AiModelInfo {
        let ready = file.isDownloaded
        return AiModelInfo(
            id: file.id, name: "\(file.name) (llama.cpp)", available: ready,
            unavailableReason: ready ? nil : "Download the model first.",
            contextTokens: Self.contextTokens)
    }

    func generate(request: AiRequest, sink: TextSink) async throws {
        guard let path = file.localURL?.path, file.isDownloaded else {
            throw AiModelError.Unavailable(reason: "Download the model first.")
        }
        try LlamaRuntime.shared.generate(
            path: path, prompt: Self.chatML(request.messages), maxTokens: Int(request.maxTokens),
            temperature: Float(request.temperature), grammar: nil, sink: sink)
    }

    /// Qwen3.5's chat format (Bonsai is built on Qwen3.5), with thinking
    /// turned off the way Qwen's own template does it, as the 1060 lab ran
    /// it ("greedy, thinking off").
    static func chatML(_ messages: [AiMessage]) -> String {
        var text = ""
        for message in messages where !message.content.isEmpty {
            let role: String
            switch message.role {
            case .system: role = "system"
            case .user: role = "user"
            case .assistant: role = "assistant"
            }
            text += "<|im_start|>\(role)\n\(message.content)<|im_end|>\n"
        }
        return text + "<|im_start|>assistant\n<think>\n\n</think>\n\n"
    }
}

/// Bonsai Harness drives the same model with a grammar: every answer must
/// be one tool call of the shapes Rust allows (rust/src/harness/calls.rs).
extension LlamaCppModel: HarnessModel {
    func generate(request: AiRequest, grammar: String?, sink: TextSink) async throws {
        guard let path = file.localURL?.path, file.isDownloaded else {
            throw AiModelError.Unavailable(reason: "Download the model first.")
        }
        try LlamaRuntime.shared.generate(
            path: path, prompt: Self.chatML(request.messages), maxTokens: Int(request.maxTokens),
            temperature: Float(request.temperature), grammar: grammar, sink: sink)
    }
}

/// The one loaded model and its context. A lock serialises loading and
/// answering; loading another file frees the previous one first, so two
/// models are never in memory together.
final class LlamaRuntime: @unchecked Sendable {
    static let shared = LlamaRuntime()

    private let lock = NSLock()
    private var path: String?
    private var model: OpaquePointer?
    private var context: OpaquePointer?
    /// How long the last load took, for the screen.
    private(set) var lastLoadSeconds: Double?

    private static let batchSize = 512

    private init() {
        llama_backend_init()
    }

    /// Frees the model if `path` (or any model, with nil) is loaded.
    func unload(path: String? = nil) {
        lock.lock()
        defer { lock.unlock() }
        if path == nil || path == self.path { freeLocked() }
    }

    /// With a `grammar` (GBNF), the answer is greedy and follows it.
    func generate(
        path: String, prompt: String, maxTokens: Int, temperature: Float, grammar: String?,
        sink: TextSink
    ) throws {
        lock.lock()
        defer { lock.unlock() }
        let (model, context) = try loadLocked(path)
        guard let vocab = llama_model_get_vocab(model) else {
            throw AiModelError.Engine(reason: "The model has no vocabulary.")
        }

        var tokens = try Self.tokenize(prompt, vocab: vocab)
        let window = Int(llama_n_ctx(context))
        guard tokens.count + maxTokens <= window else {
            throw AiModelError.ContextTooLong(
                reason: "\(tokens.count) prompt tokens + \(maxTokens) answer tokens is over \(window).")
        }

        // Each answer starts fresh: AI Lab's runs are independent.
        llama_memory_clear(llama_get_memory(context), true)

        // Read the prompt in batches, checking for Stop between them.
        var start = 0
        while start < tokens.count {
            if sink.isStopped() || Task.isCancelled { return }
            let end = min(start + Self.batchSize, tokens.count)
            let status = tokens[start..<end].withUnsafeMutableBufferPointer { buffer in
                llama_decode(context, llama_batch_get_one(buffer.baseAddress, Int32(buffer.count)))
            }
            guard status == 0 else {
                throw AiModelError.Engine(reason: "llama_decode failed reading the prompt (\(status)).")
            }
            start = end
        }
        tokens.removeAll()

        let sampler = Self.sampler(temperature: temperature)
        defer { llama_sampler_free(sampler) }
        var constraint: UnsafeMutablePointer<llama_sampler>?
        if let grammar {
            guard let parsed = llama_sampler_init_grammar(vocab, grammar, "root") else {
                throw AiModelError.Engine(reason: "llama.cpp could not read the answer's grammar.")
            }
            constraint = parsed
        }
        defer { if let constraint { llama_sampler_free(constraint) } }

        // One push per token, so AI Lab's pieces per second is tokens per
        // second. Bytes wait until they form whole UTF-8 characters.
        var pending: [UInt8] = []
        for _ in 0..<maxTokens {
            var token =
                if let constraint {
                    Self.constrainedGreedy(context: context, vocab: vocab, grammar: constraint)
                } else {
                    llama_sampler_sample(sampler, context, -1)
                }
            if llama_vocab_is_eog(vocab, token) { break }
            pending += Self.piece(token, vocab: vocab)
            if let text = String(bytes: pending, encoding: .utf8) {
                pending.removeAll()
                if !text.isEmpty, !sink.push(text: text) { return }
            } else if pending.count > 16 {
                let text = String(decoding: pending, as: UTF8.self)
                pending.removeAll()
                if !sink.push(text: text) { return }
            }
            if Task.isCancelled { return }
            let status = withUnsafeMutablePointer(to: &token) { pointer in
                llama_decode(context, llama_batch_get_one(pointer, 1))
            }
            guard status == 0 else {
                throw AiModelError.Engine(reason: "llama_decode failed while answering (\(status)).")
            }
        }
        if !pending.isEmpty {
            _ = sink.push(text: String(decoding: pending, as: UTF8.self))
        }
    }

    // MARK: Loading

    private func loadLocked(_ path: String) throws -> (OpaquePointer, OpaquePointer) {
        if path == self.path, let model, let context { return (model, context) }
        freeLocked()

        // The file is memory-mapped; refuse early rather than be killed by
        // iOS when it clearly can't fit.
        let size = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? Int64) ?? 0
        if let available = Self.availableMemory(), size > 0, available < UInt64(size) + 300_000_000 {
            throw AiModelError.Unavailable(
                reason:
                    "Not enough memory: the model needs about \(size / 1_000_000) MB and iOS gives this app \(available / 1_000_000) MB now."
            )
        }

        let started = Date()
        var modelParams = llama_model_default_params()
        modelParams.n_gpu_layers = 999  // everything on the GPU (Metal)
        guard let model = llama_model_load_from_file(path, modelParams) else {
            throw AiModelError.Engine(reason: "llama.cpp could not load the model file.")
        }
        var contextParams = llama_context_default_params()
        contextParams.n_ctx = LlamaCppModel.contextTokens
        contextParams.n_batch = UInt32(Self.batchSize)
        let cores = Int32(max(1, ProcessInfo.processInfo.activeProcessorCount - 2))
        contextParams.n_threads = cores
        contextParams.n_threads_batch = cores
        guard let context = llama_init_from_model(model, contextParams) else {
            llama_model_free(model)
            throw AiModelError.Engine(reason: "llama.cpp could not create a context (memory?).")
        }
        self.path = path
        self.model = model
        self.context = context
        lastLoadSeconds = Date().timeIntervalSince(started)
        return (model, context)
    }

    private func freeLocked() {
        if let context { llama_free(context) }
        if let model { llama_model_free(model) }
        context = nil
        model = nil
        path = nil
    }

    // MARK: Helpers

    private static func sampler(temperature: Float) -> UnsafeMutablePointer<llama_sampler> {
        let chain = llama_sampler_chain_init(llama_sampler_chain_default_params())!
        if temperature <= 0.01 {
            llama_sampler_chain_add(chain, llama_sampler_init_greedy())
        } else {
            llama_sampler_chain_add(chain, llama_sampler_init_top_p(0.95, 1))
            llama_sampler_chain_add(chain, llama_sampler_init_temp(temperature))
            llama_sampler_chain_add(chain, llama_sampler_init_dist(UInt32.random(in: 0...UInt32.max)))
        }
        return chain
    }

    /// The most likely token the grammar allows. Most tokens are allowed
    /// anyway, so the best one is checked alone first and the whole
    /// vocabulary only when it isn't (llama.cpp's own sampler does the
    /// same): with ~150k tokens, filtering every step would cost much of
    /// the phone's speed.
    private static func constrainedGreedy(
        context: OpaquePointer, vocab: OpaquePointer, grammar: UnsafeMutablePointer<llama_sampler>
    ) -> llama_token {
        let count = Int(llama_vocab_n_tokens(vocab))
        guard count > 0, let logits = llama_get_logits_ith(context, -1) else {
            return llama_vocab_eos(vocab)
        }
        var best = 0
        for index in 1..<count where logits[index] > logits[best] { best = index }
        var single = llama_token_data(id: llama_token(best), logit: logits[best], p: 0)
        let allowed = withUnsafeMutablePointer(to: &single) { pointer -> Bool in
            var array = llama_token_data_array(data: pointer, size: 1, selected: -1, sorted: false)
            llama_sampler_apply(grammar, &array)
            return pointer.pointee.logit.isFinite
        }
        var token = llama_token(best)
        if !allowed {
            var candidates = (0..<count).map {
                llama_token_data(id: llama_token($0), logit: logits[$0], p: 0)
            }
            let pick = candidates.withUnsafeMutableBufferPointer { buffer -> llama_token? in
                var array = llama_token_data_array(
                    data: buffer.baseAddress, size: buffer.count, selected: -1, sorted: false)
                llama_sampler_apply(grammar, &array)
                var pick = 0
                for index in 1..<buffer.count where buffer[index].logit > buffer[pick].logit {
                    pick = index
                }
                return buffer[pick].logit.isFinite ? buffer[pick].id : nil
            }
            // Nothing allowed (shouldn't happen): end the answer rather than
            // feed the grammar a token it rejects, which aborts llama.cpp.
            guard let pick else { return llama_vocab_eos(vocab) }
            token = pick
        }
        llama_sampler_accept(grammar, token)
        return token
    }

    private static func tokenize(_ text: String, vocab: OpaquePointer) throws -> [llama_token] {
        let length = Int32(text.utf8.count)
        var tokens = [llama_token](repeating: 0, count: Int(length) + 16)
        var count = llama_tokenize(vocab, text, length, &tokens, Int32(tokens.count), false, true)
        if count < 0 {
            tokens = [llama_token](repeating: 0, count: Int(-count))
            count = llama_tokenize(vocab, text, length, &tokens, Int32(tokens.count), false, true)
        }
        guard count >= 0 else {
            throw AiModelError.Engine(reason: "Could not tokenize the prompt.")
        }
        return Array(tokens.prefix(Int(count)))
    }

    private static func piece(_ token: llama_token, vocab: OpaquePointer) -> [UInt8] {
        var buffer = [CChar](repeating: 0, count: 64)
        var count = llama_token_to_piece(vocab, token, &buffer, Int32(buffer.count), 0, false)
        if count < 0 {
            buffer = [CChar](repeating: 0, count: Int(-count))
            count = llama_token_to_piece(vocab, token, &buffer, Int32(buffer.count), 0, false)
        }
        guard count > 0 else { return [] }
        return buffer.prefix(Int(count)).map { UInt8(bitPattern: $0) }
    }

    /// What iOS still lets this app allocate (nil off iOS).
    static func availableMemory() -> UInt64? {
        #if os(iOS)
            return UInt64(os_proc_available_memory())
        #else
            return nil
        #endif
    }
}

/// One line for AI Lab: the backends llama.cpp found (Metal on the phone).
enum LlamaProbe {
    static func describe() -> String {
        _ = LlamaRuntime.shared  // initialises the backends once
        var devices: [String] = []
        for index in 0..<ggml_backend_dev_count() {
            guard let device = ggml_backend_dev_get(index) else { continue }
            devices.append(String(cString: ggml_backend_dev_name(device)))
        }
        return devices.isEmpty ? "No backends" : devices.joined(separator: ", ")
    }
}
