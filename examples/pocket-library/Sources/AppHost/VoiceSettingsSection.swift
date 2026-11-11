@preconcurrency import AVFoundation
@preconcurrency import PocketLibraryFFI
import ForgeUI
import SwiftUI

/// Settings > Read Aloud: which voice reads books, a sample to hear it,
/// and where to get better voices. The reader's own menu changes the same
/// setting.
struct VoiceSettingsSection: View {
    @AppStorage(ReadAloud.voiceKey) private var voiceID: String?
    @State private var voices: [VoiceCandidate] = []
    @State private var canAskForPersonalVoice = false
    @State private var sampler = VoiceSampler()
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Section {
            Picker("Voice", selection: $voiceID) {
                Text(bestLabel).tag(String?.none)
                ForEach(voices) { voice in
                    Text(VoiceChoice.label(for: voice)).tag(String?.some(voice.id))
                }
            }
            #if os(iOS)
            // A pushed list on the phone; macOS (the host tests) has no
            // navigation-link picker style.
            .pickerStyle(.navigationLink)
            #endif
            .accessibilityIdentifier("settings.voice")

            Button(sampler.playing ? "Stop Sample" : "Play Sample") {
                sampler.playing ? sampler.stop() : sampler.play(voiceID: voiceID)
            }
            .accessibilityIdentifier("settings.voiceSample")

            if canAskForPersonalVoice {
                Button("Use My Personal Voice…") {
                    Task {
                        _ = await ReadAloud.askForPersonalVoice()
                        refresh()
                    }
                }
                .accessibilityIdentifier("settings.personalVoice")
            }
        } header: {
            Text("Read Aloud")
        } footer: {
            if VoiceChoice.onlyStandard(voices, language: ReadAloud.language) {
                Text("Only the basic voices are installed, and they sound robotic. For a natural voice, download an Enhanced or Premium voice in \(VoiceChoice.downloadHint), then come back here.")
            } else {
                Text("Best Available uses the most natural voice installed. Download more in \(VoiceChoice.downloadHint).")
            }
        }
        .onAppear(perform: refresh)
        .onChange(of: scenePhase) { _, phase in
            // Voices downloaded in the Settings app show up on return.
            if phase == .active { refresh() }
        }
        .onChange(of: voiceID) { _, _ in sampler.stop() }
        .onDisappear { sampler.stop() }
    }

    private var bestLabel: String {
        VoiceChoice.best(voices, language: ReadAloud.language).map { "Best Available (\($0.name))" }
            ?? "Best Available"
    }

    private func refresh() {
        voices = ReadAloud.voices()
        canAskForPersonalVoice = ReadAloud.canAskForPersonalVoice
        // A picked voice that has since been deleted falls back to the best.
        if let voiceID, !voices.contains(where: { $0.id == voiceID }) {
            self.voiceID = nil
        }
    }
}

/// Speaks one sentence in a voice, so the user can hear it before reading
/// a book with it.
@Observable
@MainActor
final class VoiceSampler: NSObject, AVSpeechSynthesizerDelegate {
    private(set) var playing = false
    @ObservationIgnored private let synthesizer = AVSpeechSynthesizer()

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    func play(voiceID: String?) {
        stop()
        let utterance = AVSpeechUtterance(
            string: "This is how your books will sound when Pocket Library reads them aloud."
        )
        utterance.voice = voiceID.flatMap(AVSpeechSynthesisVoice.init(identifier:)) ?? ReadAloud.bestVoice()
        try? ForgeAudioSession.activate(.playback)
        playing = true
        synthesizer.speak(utterance)
    }

    func stop() {
        guard playing else { return }
        synthesizer.stopSpeaking(at: .immediate)
        finish()
    }

    private func finish() {
        guard playing else { return }
        playing = false
        ForgeAudioSession.deactivate()
    }

    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        Task { @MainActor in self.finish() }
    }
}
