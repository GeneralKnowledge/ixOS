@preconcurrency import AVFoundation
@preconcurrency import UrduNaturallyFFI
import Foundation
import Observation

/// Reads Urdu aloud with the phone's own voices: men's and women's, at
/// natural or slow pace. It reads the phrase's Urdu script (`speech`), never
/// the Roman Urdu, which no voice pronounces properly.
///
/// Which Urdu voices a phone has varies; Speech Check (Settings) lists them.
/// When there is none the app still works as text, and says so.
@Observable
@MainActor
final class Narrator: NSObject {
    static let shared = Narrator()

    enum Pace {
        case natural
        case slow
    }

    /// The phrase being read, so its play button can show it.
    private(set) var speakingID: String?

    @ObservationIgnored private let synthesizer = AVSpeechSynthesizer()
    /// The utterance being read; a late callback for an older one is ignored.
    @ObservationIgnored private var current: ObjectIdentifier?

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    /// Every installed Urdu voice, best quality first.
    static var urduVoices: [AVSpeechSynthesisVoice] {
        AVSpeechSynthesisVoice.speechVoices()
            .filter { $0.language.hasPrefix("ur") }
            .sorted { $0.quality.rawValue > $1.quality.rawValue }
    }

    static var hasUrduVoice: Bool { !urduVoices.isEmpty }

    /// The best voice of `gender`, else the best Urdu voice of any kind.
    static func voice(for gender: Gender) -> AVSpeechSynthesisVoice? {
        let voices = urduVoices
        let wanted: AVSpeechSynthesisVoiceGender = gender == .female ? .female : .male
        return voices.first { $0.gender == wanted } ?? voices.first
    }

    func say(_ phrase: PhraseView, voice gender: Gender = .male, rate: Float = 1, pace: Pace = .natural) {
        say(text: phrase.speech, id: phrase.id, voice: gender, rate: rate, pace: pace)
    }

    func say(text: String, id: String, voice gender: Gender = .male, rate: Float = 1, pace: Pace = .natural) {
        synthesizer.stopSpeaking(at: .immediate)
        guard !text.isEmpty else { return }
        try? ForgeAudioSession.activate(.playback)
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = Self.voice(for: gender) ?? AVSpeechSynthesisVoice(language: "ur-PK")
        let slow: Float = pace == .slow ? 0.6 : 1
        utterance.rate = min(
            max(AVSpeechUtteranceDefaultSpeechRate * rate * slow, AVSpeechUtteranceMinimumSpeechRate),
            AVSpeechUtteranceMaximumSpeechRate)
        speakingID = id
        current = ObjectIdentifier(utterance)
        synthesizer.speak(utterance)
    }

    func stop() {
        current = nil
        synthesizer.stopSpeaking(at: .immediate)
        speakingID = nil
        ForgeAudioSession.deactivate()
    }

    fileprivate func ended(_ utterance: ObjectIdentifier) {
        guard current == utterance else { return }
        current = nil
        speakingID = nil
        ForgeAudioSession.deactivate()
    }
}

extension Narrator: AVSpeechSynthesizerDelegate {
    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        let id = ObjectIdentifier(utterance)
        Task { @MainActor in self.ended(id) }
    }

    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) {
        let id = ObjectIdentifier(utterance)
        Task { @MainActor in self.ended(id) }
    }
}
