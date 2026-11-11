@preconcurrency import CoreMLLLM
@preconcurrency import AICheckFFI
import ForgeKit
import Foundation
import SwiftUI
import Tokenizers
import os
#if canImport(FoundationModels)
    import FoundationModels
#endif
#if canImport(UIKit)
    import UIKit
#endif

// Written by scripts/make-ai-check-app.sh in place of the starter screen: the
// local AI spike (L0 in docs/local-ai.md). It answers four questions on a
// real iPhone signed with a free team: does Apple Foundation Models work, does
// CoreML-LLM download, compile and run, how much memory is there, and how fast
// does generated text reach Rust. Every result line also goes to the system
// log (`forge device logs --grep ai-check`).

// MARK: - Device readings

enum DeviceReadings {
    /// Memory this process may still allocate before iOS ends it, in MB.
    static func availableMB() -> Int? {
        #if os(iOS)
            return Int(os_proc_available_memory() / 1_048_576)
        #else
            return nil
        #endif
    }

    /// The memory iOS charges to this app (what jetsam compares), in MB.
    static func footprintMB() -> Int? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(
            MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        guard result == KERN_SUCCESS else { return nil }
        return Int(info.phys_footprint / 1_048_576)
    }

    static func freeDiskMB() -> Int? {
        let url = URL(fileURLWithPath: NSHomeDirectory())
        let values = try? url.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        return values?.volumeAvailableCapacityForImportantUsage.map { Int($0 / 1_048_576) }
    }

    static func thermal() -> String {
        switch ProcessInfo.processInfo.thermalState {
        case .nominal: return "nominal"
        case .fair: return "fair"
        case .serious: return "serious"
        case .critical: return "critical"
        @unknown default: return "unknown"
        }
    }

    static func battery() -> String {
        #if os(iOS)
            UIDevice.current.isBatteryMonitoringEnabled = true
            let level = UIDevice.current.batteryLevel
            let lowPower = ProcessInfo.processInfo.isLowPowerModeEnabled ? ", Low Power Mode" : ""
            return level < 0 ? "unknown\(lowPower)" : "\(Int(level * 100))%\(lowPower)"
        #else
            return "n/a"
        #endif
    }

    static func model() -> String {
        var info = utsname()
        uname(&info)
        return withUnsafeBytes(of: &info.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
    }

    static func summary() -> String {
        func mb(_ v: Int?) -> String { v.map { "\($0) MB" } ?? "n/a" }
        return "device \(model()), iOS \(ProcessInfo.processInfo.operatingSystemVersionString); "
            + "available \(mb(availableMB())), footprint \(mb(footprintMB())), "
            + "free disk \(mb(freeDiskMB())), thermal \(thermal()), battery \(battery())"
    }
}

// MARK: - Engines

/// A downloadable model the spike can try.
struct CheckModel: Identifiable, Hashable {
    let id: String
    let name: String
    let size: String
    /// Qwen3.5 runs through `Qwen35MLKVGenerator`; the rest through `CoreMLLLM`.
    let isQwen35: Bool
    let tokenizer: String?

    static let all = [
        CheckModel(id: "lfm2.5-350m", name: "LFM2.5 350M", size: "810 MB", isQwen35: false, tokenizer: nil),
        CheckModel(id: "qwen3.5-0.8b", name: "Qwen3.5 0.8B", size: "1.2 GB", isQwen35: true, tokenizer: "Qwen/Qwen3.5-0.8B"),
        CheckModel(id: "qwen3.5-2b", name: "Qwen3.5 2B", size: "2.8 GB", isQwen35: true, tokenizer: "Qwen/Qwen3.5-2B"),
    ]

    var info: ModelDownloader.ModelInfo? {
        ModelDownloader.ModelInfo.defaults.first { $0.id == id }
    }
}

enum EngineChoice: String, CaseIterable, Identifiable {
    case foundation = "Apple Foundation Models"
    case downloaded = "Downloaded model"
    var id: String { rawValue }
}

/// Holds the loaded CoreML-LLM model. Lives on the main actor; generation
/// runs off it.
@MainActor
@Observable
final class ModelHost {
    var loadedId: String?
    var llm: CoreMLLLM?
    var qwen: StoppableQwen35Generator?
    var qwenTokenizer: (any Tokenizer)?

    func unload() {
        llm = nil
        qwen = nil
        qwenTokenizer = nil
        loadedId = nil
    }
}

enum CheckError: LocalizedError {
    case notLoaded, unavailable(String)
    var errorDescription: String? {
        switch self {
        case .notLoaded: return "No downloaded model is loaded"
        case .unavailable(let why): return why
        }
    }
}

/// Generates an answer for `turns` with the chosen engine, pushing text into
/// `sink` until it finishes or the sink says stop.
@MainActor
func generate(
    engine: EngineChoice, host: ModelHost, turns: [AiTurn], maxTokens: Int,
    temperature: Double, sink: AiTokenSink, onPiece: @escaping @Sendable () -> Void
) async throws {
    switch engine {
    case .foundation:
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                try await generateWithFoundationModels(
                    turns: turns, maxTokens: maxTokens, temperature: temperature, sink: sink, onPiece: onPiece)
                return
            }
        #endif
        throw CheckError.unavailable("Foundation Models needs iOS 26 and an SDK that has it")
    case .downloaded:
        if let llm = host.llm {
            let messages = turns.map { turn in
                CoreMLLLM.Message(role: CoreMLLLM.Message.Role(rawValue: turn.role) ?? .user, content: turn.text)
            }
            let stream = try await llm.stream(messages, maxTokens: maxTokens)
            for await piece in stream {
                onPiece()
                if !sink.push(text: piece) || Task.isCancelled { break }
            }
        } else if let qwen = host.qwen, let tok = host.qwenTokenizer {
            try await generateWithQwen(
                qwen: qwen, tokenizer: tok, turns: turns, maxTokens: maxTokens,
                temperature: temperature, sink: sink, onPiece: onPiece)
        } else {
            throw CheckError.notLoaded
        }
    }
}

#if canImport(FoundationModels)
    @available(iOS 26.0, macOS 26.0, *)
    func foundationModelsAvailability() -> String {
        switch SystemLanguageModel.default.availability {
        case .available:
            return "available"
        case .unavailable(let reason):
            return "unavailable: \(reason)"
        }
    }

    /// Foundation Models keeps its own history inside a session, but Rust owns
    /// ours: each answer gets a fresh session whose instructions are the system
    /// turn and whose prompt is the rest of the conversation. The stream yields
    /// the whole answer so far each time; only the new part goes to Rust.
    @available(iOS 26.0, macOS 26.0, *)
    func generateWithFoundationModels(
        turns: [AiTurn], maxTokens: Int, temperature: Double, sink: AiTokenSink,
        onPiece: @escaping @Sendable () -> Void
    ) async throws {
        let instructions = turns.first { $0.role == "system" }?.text ?? ""
        let rest = turns.filter { $0.role != "system" }
        let prompt: String
        if rest.count == 1 {
            prompt = rest[0].text
        } else {
            prompt = rest.map { ($0.role == "user" ? "User: " : "You: ") + $0.text }
                .joined(separator: "\n\n") + "\n\nYou:"
        }
        let session = LanguageModelSession(instructions: instructions)
        let options = GenerationOptions(temperature: temperature, maximumResponseTokens: maxTokens)
        var sent = ""
        for try await snapshot in session.streamResponse(to: prompt, options: options) {
            let whole = snapshot.content
            guard whole.count > sent.count, whole.hasPrefix(sent) else { continue }
            let piece = String(whole.dropFirst(sent.count))
            sent = whole
            onPiece()
            if !sink.push(text: piece) || Task.isCancelled { break }
        }
    }
#endif

/// Qwen3.5 runs through its own generator with a token callback. CoreML-LLM's
/// version cannot be stopped, so the check app builds a patched copy
/// (`StoppableQwen35Generator`) that checks `shouldStop` and task cancellation
/// before every step: Stop really ends the computation, and the report's
/// stop latency is at most one step.
func generateWithQwen(
    qwen: StoppableQwen35Generator, tokenizer: any Tokenizer, turns: [AiTurn], maxTokens: Int,
    temperature: Double, sink: AiTokenSink, onPiece: @escaping @Sendable () -> Void
) async throws {
    let chat: [[String: Any]] = turns.map { ["role": $0.role, "content": $0.text] }
    // Never drop the system prompt or the history: if swift-transformers
    // cannot render Qwen3.5's template, write the same ChatML by hand.
    let ids: [Int]
    do {
        ids = try tokenizer.applyChatTemplate(messages: chat)
    } catch {
        aiLog(line: "INFO qwen: chat template failed (\(error.localizedDescription)); using hand-written ChatML with the whole conversation")
        let chatML = turns.map { "<|im_start|>\($0.role)\n\($0.text)<|im_end|>\n" }.joined()
            + "<|im_start|>assistant\n"
        ids = tokenizer.encode(text: chatML)
    }
    let room = 2048 - ids.count - 1
    guard room > 0 else { throw CheckError.unavailable("prompt is \(ids.count) tokens, over 2048") }
    var eos: Set<Int32> = [248044, 248045, 248046]
    if let id = tokenizer.eosTokenId { eos.insert(Int32(id)) }
    var ids32: [Int] = []
    var sent = ""
    qwen.shouldStop = { sink.isStopped() }
    defer { qwen.shouldStop = nil }
    _ = try await qwen.generate(
        inputIds: ids.map { Int32($0) }, maxNewTokens: min(room, maxTokens),
        temperature: Float(temperature), topK: 40, topP: 0.95, repetitionPenalty: 1.1,
        eosTokenIds: eos,
        onToken: { token in
            if eos.contains(token) { return }
            ids32.append(Int(token))
            var text = tokenizer.decode(tokens: ids32)
            while text.hasSuffix("\u{FFFD}") { text = String(text.dropLast()) }
            guard text.count > sent.count, text.hasPrefix(sent) else { return }
            let piece = String(text.dropFirst(sent.count))
            sent = text
            onPiece()
            _ = sink.push(text: piece)
        })
}

// MARK: - The screen

public struct ContentView: View {
    public init() {}
    /// The starter's entry points pass their ForgeKit host; the check
    /// screen doesn't use the app's Rust core, so it ignores it.
    public init(host _: ForgeHost<AppCore>) {}

    @State private var log: [String] = []
    @State private var host = ModelHost()
    @State private var downloader = ModelDownloader.shared
    @State private var selectedModel = CheckModel.all[1]
    @State private var busy: String?
    @State private var fmStatus = "not checked"

    // Generation
    @State private var engine: EngineChoice = .downloaded
    @State private var prompt = "Explain in two sentences why the sky is blue."
    @State private var answer = ""
    @State private var maxTokens = 200
    @State private var temperature = 0.7
    @State private var autoStopAfter = 0
    @State private var sink: AiTokenSink?
    @State private var generation: Task<Void, Never>?
    @State private var stopPressedAt: Date?

    // Characters
    @State private var characterId = "girlfriend"
    @State private var remind = true
    @State private var history: [AiTurn] = []
    @State private var message = ""

    public var body: some View {
        NavigationStack {
            Form {
                deviceSection
                foundationSection
                modelSection
                generateSection
                characterSection
                pipeSection
                resultsSection
            }
            .navigationTitle("Local AI check")
        }
        .task { record("INFO start: " + DeviceReadings.summary()) }
    }

    // 1. Device
    private var deviceSection: some View {
        Section("1. Device") {
            Button("Record memory, disk, thermal and battery") {
                record("INFO device: " + DeviceReadings.summary())
            }
        }
    }

    // 2. Foundation Models
    private var foundationSection: some View {
        Section("2. Apple Foundation Models") {
            Text("Status: \(fmStatus)").font(.footnote)
            Button("Check availability") { checkFoundationModels() }
        }
    }

    // 3. Downloaded model
    private var modelSection: some View {
        Section("3. Downloadable model (CoreML-LLM)") {
            Picker("Model", selection: $selectedModel) {
                ForEach(CheckModel.all) { Text("\($0.name) (\($0.size))").tag($0) }
            }
            if let info = selectedModel.info {
                let present = downloader.isDownloaded(info)
                Text(present ? "Downloaded" : "Not downloaded").font(.footnote)
                if downloader.isDownloading {
                    ProgressView(value: downloader.progress) { Text(downloader.status).font(.footnote) }
                    Button(downloader.isPaused ? "Resume" : "Pause") {
                        downloader.isPaused ? downloader.resumeDownload() : downloader.pause()
                    }
                    Button("Cancel download", role: .destructive) { downloader.cancelDownload() }
                } else if !present {
                    Button("Download") { download(info) }.disabled(busy != nil)
                } else {
                    Button(host.loadedId == selectedModel.id ? "Loaded" : "Load") { load(selectedModel, info) }
                        .disabled(busy != nil || host.loadedId == selectedModel.id)
                    if host.loadedId == selectedModel.id {
                        Button("Unload") {
                            host.unload()
                            record("INFO unload \(selectedModel.id): " + DeviceReadings.summary())
                        }
                    }
                    Button("Delete download", role: .destructive) { delete(info) }.disabled(busy != nil)
                }
            } else {
                Text("This CoreML-LLM version does not list \(selectedModel.id)").foregroundStyle(.red)
            }
            if let busy { Text(busy).font(.footnote).foregroundStyle(.secondary) }
        }
    }

    // 4. Generate
    private var generateSection: some View {
        Section("4. Generate and stop") {
            Picker("Engine", selection: $engine) {
                ForEach(EngineChoice.allCases) { Text($0.rawValue).tag($0) }
            }
            TextField("Prompt", text: $prompt, axis: .vertical)
            Stepper("Max tokens: \(maxTokens)", value: $maxTokens, in: 16...1024, step: 16)
            HStack {
                Text("Temperature \(temperature, specifier: "%.1f")")
                Slider(value: $temperature, in: 0...1.5, step: 0.1)
            }
            Stepper(
                autoStopAfter == 0 ? "Auto-stop: off" : "Auto-stop after \(autoStopAfter) pieces",
                value: $autoStopAfter, in: 0...200, step: 10)
            if generation == nil {
                Button("Generate") {
                    let turns = aiBuildPrompt(
                        characterId: "assistant", history: [], userText: prompt, remind: false, budgetChars: 6000)
                    run(label: "generate", turns: turns) { answer = $0 }
                }
            } else {
                Button("Stop", role: .destructive) { stop() }
            }
            if !answer.isEmpty { Text(answer).font(.callout) }
        }
    }

    // 5. Characters
    private var characterSection: some View {
        Section("5. Character (\(engine.rawValue))") {
            Picker("Character", selection: $characterId) {
                ForEach(aiCharacters(), id: \.id) { Text("\($0.emoji) \($0.name): \($0.description)").tag($0.id) }
            }
            .onChange(of: characterId) { history = [] }
            Toggle("Remind the model of the character each turn", isOn: $remind)
            ForEach(Array(history.enumerated()), id: \.offset) { _, turn in
                Text((turn.role == "user" ? "You: " : "") + turn.text)
                    .font(.callout)
                    .foregroundStyle(turn.role == "user" ? Color.secondary : Color.primary)
            }
            HStack {
                TextField("Message", text: $message)
                Button("Send") { sendToCharacter() }.disabled(message.isEmpty || generation != nil)
            }
            Button("Clear conversation") { history = [] }.disabled(history.isEmpty)
            Text("Turn \(history.count / 2). Rate each reply in character or not; ten turns is the test.")
                .font(.footnote).foregroundStyle(.secondary)
        }
    }

    // 6. Swift → Rust pipe
    private var pipeSection: some View {
        Section("6. Swift → Rust text pipe") {
            Button("Push 10,000 pieces into Rust") { pipeBenchmark() }
        }
    }

    // Results
    private var resultsSection: some View {
        Section("Results (also in the system log)") {
            ShareLink(item: log.joined(separator: "\n")) { Label("Share results", systemImage: "square.and.arrow.up") }
            ForEach(Array(log.enumerated().reversed()), id: \.offset) { _, line in
                Text(line)
                    .font(.footnote.monospaced())
                    .foregroundStyle(line.hasPrefix("FAIL") ? Color.red : Color.primary)
                    .textSelection(.enabled)
            }
        }
    }

    // MARK: Actions

    private func record(_ line: String) {
        log.append(line)
        aiLog(line: line)
    }

    private func checkFoundationModels() {
        #if canImport(FoundationModels)
            if #available(iOS 26.0, macOS 26.0, *) {
                fmStatus = foundationModelsAvailability()
                record((fmStatus == "available" ? "PASS" : "FAIL") + " foundation-models: \(fmStatus)")
                return
            }
        #endif
        fmStatus = "not in this build (needs iOS 26 and the iOS 26 SDK)"
        record("FAIL foundation-models: \(fmStatus)")
    }

    private func download(_ info: ModelDownloader.ModelInfo) {
        let started = Date()
        busy = "Downloading \(info.name)…"
        record("INFO download \(info.id) start: " + DeviceReadings.summary())
        Task {
            do {
                let url = try await downloader.download(info)
                excludeFromBackup(url.deletingLastPathComponent())
                record(String(format: "PASS download %@: %.0f s, ", info.id, Date().timeIntervalSince(started))
                    + DeviceReadings.summary())
            } catch {
                record("FAIL download \(info.id): \(error.localizedDescription)")
            }
            busy = nil
        }
    }

    /// CoreML-LLM keeps models in Documents/Models, which iCloud backs up.
    private func excludeFromBackup(_ url: URL) {
        var dir = url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? dir.setResourceValues(values)
    }

    private func delete(_ info: ModelDownloader.ModelInfo) {
        if host.loadedId == info.id { host.unload() }
        do {
            try downloader.delete(info)
            record("PASS delete \(info.id): " + DeviceReadings.summary())
        } catch {
            record("FAIL delete \(info.id): \(error.localizedDescription)")
        }
    }

    private func load(_ model: CheckModel, _ info: ModelDownloader.ModelInfo) {
        host.unload()
        let started = Date()
        let before = DeviceReadings.availableMB()
        busy = "Loading \(model.name) (the first load compiles it, which can take minutes)…"
        record("INFO load \(model.id) start: " + DeviceReadings.summary())
        Task {
            do {
                guard let file = downloader.localModelURL(for: info) else { throw CheckError.notLoaded }
                let folder = file.deletingLastPathComponent()
                if model.isQwen35, let tokenizerId = model.tokenizer {
                    let tokenizer = try await AutoTokenizer.from(pretrained: tokenizerId)
                    let generator = StoppableQwen35Generator(cfg: model.id == "qwen3.5-2b" ? .default2B : .default0_8B)
                    generator.setModelFolder(folder)
                    try await generator.load()
                    host.qwen = generator
                    host.qwenTokenizer = tokenizer
                } else {
                    host.llm = try await CoreMLLLM.load(from: folder) { status in
                        Task { @MainActor in busy = status }
                    }
                }
                host.loadedId = model.id
                var used = "n/a"
                if let before, let after = DeviceReadings.availableMB() { used = "\(before - after) MB" }
                record(String(format: "PASS load %@: %.1f s, available memory used %@; ", model.id,
                              Date().timeIntervalSince(started), used) + DeviceReadings.summary())
            } catch {
                record("FAIL load \(model.id): \(error.localizedDescription); " + DeviceReadings.summary())
            }
            busy = nil
        }
    }

    private func run(label: String, turns: [AiTurn], onText: @escaping (String) -> Void) {
        let sink = AiTokenSink(stopAfterPieces: UInt32(autoStopAfter))
        self.sink = sink
        stopPressedAt = nil
        onText("")
        let engineName = engine == .foundation ? "foundation-models" : (host.loadedId ?? "none")
        let started = Date()
        // The UI reads the text from Rust about 10 times a second, not per token.
        let refresher = Task { @MainActor in
            while !Task.isCancelled {
                onText(sink.text())
                try? await Task.sleep(nanoseconds: 100_000_000)
            }
        }
        let footprintBefore = DeviceReadings.footprintMB() ?? 0
        let task = Task { @MainActor in
            var failure: String?
            do {
                try await generate(
                    engine: engine, host: host, turns: turns, maxTokens: maxTokens,
                    temperature: temperature, sink: sink, onPiece: {})
            } catch is CancellationError {
            } catch {
                failure = error.localizedDescription
            }
            refresher.cancel()
            onText(sink.text())
            let footprintAfter = max(footprintBefore, DeviceReadings.footprintMB() ?? 0)
            let r = sink.report()
            let stopLatency = stopPressedAt.map { String(format: ", stop took %.2f s", Date().timeIntervalSince($0)) } ?? ""
            let first = r.firstPieceMs.map { "\($0) ms" } ?? "none"
            let line = String(
                format: "%@ %@ %@: first text %@, %.1f pieces/s, %ld pieces, %ld chars, total %.1f s, %ld late pieces%@, footprint after %ld MB; ",
                failure == nil ? "PASS" : "FAIL", label, engineName, first, r.piecesPerSecond, Int(r.pieces),
                Int(r.chars), Date().timeIntervalSince(started), Int(r.piecesAfterStop), stopLatency, footprintAfter)
            record(line + (failure.map { "error: \($0); " } ?? "") + DeviceReadings.summary())
            generation = nil
            self.sink = nil
        }
        generation = task
    }

    private func stop() {
        stopPressedAt = Date()
        sink?.stop()
        generation?.cancel()
    }

    private func sendToCharacter() {
        let text = message
        message = ""
        let turns = aiBuildPrompt(
            characterId: characterId, history: history, userText: text, remind: remind, budgetChars: 6000)
        history.append(AiTurn(role: "user", text: text))
        history.append(AiTurn(role: "assistant", text: ""))
        let index = history.count - 1
        run(label: "character-\(characterId)-turn-\(history.count / 2)", turns: turns) { reply in
            if index < history.count { history[index] = AiTurn(role: "assistant", text: reply) }
        }
    }

    private func pipeBenchmark() {
        Task.detached {
            let sink = AiTokenSink(stopAfterPieces: 0)
            let count = 10_000
            let start = Date()
            for i in 0..<count { _ = sink.push(text: i % 2 == 0 ? "tok" : "en ") }
            let perPush = Date().timeIntervalSince(start) / Double(count) * 1_000_000
            let line = String(format: "PASS pipe: %ld pushes, %.1f µs per push (Swift → Rust, with the text)", count, perPush)
            await MainActor.run { record(line) }
        }
    }
}
