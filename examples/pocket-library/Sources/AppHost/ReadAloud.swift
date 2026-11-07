@preconcurrency import AVFoundation
@preconcurrency import ForgeFFI
import Foundation
import Observation
#if os(iOS)
import MediaPlayer
import UIKit
#endif

/// Reads a chapter aloud with the system's on-device voices, one block
/// (paragraph, heading, quote…) per utterance, and reports the block and
/// word being spoken so the reader can follow and highlight them. Keeps
/// playing with the screen locked (the `audio` background mode, in
/// forge.toml) and answers the lock screen's play, pause and skip buttons.
///
/// Speech is presentation, not a rule, so it lives in Swift: Rust has
/// already turned the book into blocks, and the reader saves the position
/// as the voice moves through them.
@Observable
@MainActor
final class ReadAloud: NSObject {
    enum State: Equatable {
        case stopped
        case playing
        case paused
    }

    private(set) var state: State = .stopped
    /// The block being read, as an index into the chapter's blocks.
    private(set) var block: Int?
    /// The word being read, in the block's text.
    private(set) var word: NSRange?

    /// 0.5×–3×, where 1× is the voice's normal pace.
    var speed: Double = ReadAloud.savedSpeed {
        didSet {
            UserDefaults.standard.set(speed, forKey: Self.speedKey)
            restartBlock()
        }
    }

    /// A voice identifier, or `nil` for the default voice of the device's
    /// language.
    var voiceID: String? = UserDefaults.standard.string(forKey: ReadAloud.voiceKey) {
        didSet {
            UserDefaults.standard.set(voiceID, forKey: Self.voiceKey)
            restartBlock()
        }
    }

    /// Called with each block as reading reaches it (also in the
    /// background), so the reader can save its place.
    @ObservationIgnored var onBlock: (@MainActor (Int) -> Void)?

    /// Called when the last block of a chapter has been read, so the
    /// reader can load the next one and call `play(from: 0)`.
    @ObservationIgnored var onChapterEnd: (@MainActor () -> Void)?

    @ObservationIgnored private let synthesizer = AVSpeechSynthesizer()
    @ObservationIgnored private var texts: [String] = []
    @ObservationIgnored private var utterance: AVSpeechUtterance?
    @ObservationIgnored private var bookTitle = ""
    @ObservationIgnored private var author = ""
    @ObservationIgnored private var chapterTitle = ""
    @ObservationIgnored private var coverFile: String?
    @ObservationIgnored private var remoteCommandsAdded = false

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    // MARK: Content

    /// The book being read, for the lock screen.
    func setBook(title: String, author: String, coverFile: String?) {
        bookTitle = title
        self.author = author
        self.coverFile = coverFile
    }

    /// The chapter to read. Stops reading the previous one; the state is
    /// kept, so a reader moving on from the end of a chapter just calls
    /// `play(from: 0)`.
    func load(_ chapter: ReaderChapter) {
        texts = chapter.blocks.map(Self.speakableText)
        chapterTitle = chapter.title
        word = nil
        block = nil
        utterance = nil
        if synthesizer.isSpeaking { synthesizer.stopSpeaking(at: .immediate) }
        updateNowPlaying()
    }

    /// The text spoken for a block: exactly the text the reader shows, so
    /// word ranges line up with it. Scene breaks and pictures are skipped.
    static func speakableText(_ block: TextBlock) -> String {
        switch block.kind {
        case .separator, .image: return ""
        default: return block.spans.map(\.text).joined()
        }
    }

    // MARK: Controls

    /// Starts reading at block `index` (or the next block with text).
    func play(from index: Int) {
        activateAudioSession()
        addRemoteCommands()
        state = .playing
        speak(from: index)
    }

    func pause() {
        guard state == .playing else { return }
        synthesizer.pauseSpeaking(at: .word)
        state = .paused
        updateNowPlaying()
    }

    func resume() {
        guard state == .paused else { return }
        state = .playing
        if synthesizer.isPaused {
            synthesizer.continueSpeaking()
        } else {
            speak(from: block ?? 0)
        }
        updateNowPlaying()
    }

    /// Play from `index` when stopped, otherwise pause or resume.
    func toggle(from index: Int) {
        switch state {
        case .stopped: play(from: index)
        case .playing: pause()
        case .paused: resume()
        }
    }

    /// The previous or next block with text.
    func skip(by offset: Int) {
        guard let current = block else { return }
        var target = current + offset
        while target >= 0, target < texts.count, texts[target].isEmpty {
            target += offset
        }
        guard target >= 0 else { return }
        state = .playing
        activateAudioSession()
        speak(from: target)
    }

    func stop() {
        state = .stopped
        utterance = nil
        word = nil
        block = nil
        if synthesizer.isSpeaking || synthesizer.isPaused {
            synthesizer.stopSpeaking(at: .immediate)
        }
        clearNowPlaying()
        #if os(iOS)
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        #endif
    }

    // MARK: Voices

    /// The voices for the device's language (or English), best quality
    /// first. Better voices can be downloaded in Settings > Accessibility >
    /// Spoken Content > Voices.
    static func voices() -> [AVSpeechSynthesisVoice] {
        let language = AVSpeechSynthesisVoice.currentLanguageCode()
        let prefix = String(language.prefix(2))
        let all = AVSpeechSynthesisVoice.speechVoices()
        var matching = all.filter { $0.language.hasPrefix(prefix) }
        if matching.isEmpty { matching = all.filter { $0.language.hasPrefix("en") } }
        return matching.sorted {
            ($0.quality.rawValue, $1.name) > ($1.quality.rawValue, $0.name)
        }
    }

    static func label(for voice: AVSpeechSynthesisVoice) -> String {
        let region = Locale.current.localizedString(forIdentifier: voice.language) ?? voice.language
        switch voice.quality {
        case .premium: return "\(voice.name) (Premium, \(region))"
        case .enhanced: return "\(voice.name) (Enhanced, \(region))"
        default: return "\(voice.name) (\(region))"
        }
    }

    // MARK: Speaking

    private func speak(from index: Int) {
        var index = max(index, 0)
        while index < texts.count, texts[index].isEmpty { index += 1 }
        utterance = nil
        if synthesizer.isSpeaking || synthesizer.isPaused {
            synthesizer.stopSpeaking(at: .immediate)
        }
        guard index < texts.count else {
            block = nil
            word = nil
            if let onChapterEnd {
                onChapterEnd()
            } else {
                stop()
            }
            return
        }
        let next = AVSpeechUtterance(string: texts[index])
        next.rate = Self.rate(for: speed)
        next.voice = voiceID.flatMap(AVSpeechSynthesisVoice.init(identifier:))
            ?? AVSpeechSynthesisVoice(language: AVSpeechSynthesisVoice.currentLanguageCode())
        next.postUtteranceDelay = 0.15
        utterance = next
        block = index
        word = nil
        synthesizer.speak(next)
        onBlock?(index)
        updateNowPlaying()
    }

    /// Speed changes and new voices apply from the start of the block.
    private func restartBlock() {
        guard state == .playing, let block else { return }
        speak(from: block)
    }

    /// AVSpeech rates run from 0 to 1 with 0.5 as normal, and are not
    /// linear: about 0.6 already sounds twice as fast.
    static func rate(for speed: Double) -> Float {
        let normal = Double(AVSpeechUtteranceDefaultSpeechRate)
        let rate: Double
        if speed <= 1 {
            rate = normal * max(speed, 0.5)
        } else {
            let span = Double(AVSpeechUtteranceMaximumSpeechRate) - normal
            rate = normal + span * min(log2(speed) / 2.5, 1)
        }
        return Float(rate)
    }

    private static let speedKey = "readAloudSpeed"
    private static let voiceKey = "readAloudVoice"
    private static var savedSpeed: Double {
        let saved = UserDefaults.standard.double(forKey: speedKey)
        return saved > 0 ? saved : 1
    }

    fileprivate func finished(_ id: ObjectIdentifier) {
        guard let utterance, ObjectIdentifier(utterance) == id, state == .playing else { return }
        speak(from: (block ?? -1) + 1)
    }

    fileprivate func willSpeak(_ range: NSRange, in id: ObjectIdentifier) {
        guard let utterance, ObjectIdentifier(utterance) == id else { return }
        word = range
    }

    // MARK: System integration

    private func activateAudioSession() {
        #if os(iOS)
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .spokenAudio)
        try? session.setActive(true)
        #endif
    }

    private func addRemoteCommands() {
        #if os(iOS)
        guard !remoteCommandsAdded else { return }
        remoteCommandsAdded = true
        let center = MPRemoteCommandCenter.shared()
        center.playCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in self?.resume() }
            return .success
        }
        center.pauseCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in self?.pause() }
            return .success
        }
        center.togglePlayPauseCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.state == .playing ? self.pause() : self.resume()
            }
            return .success
        }
        center.nextTrackCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in self?.skip(by: 1) }
            return .success
        }
        center.previousTrackCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in self?.skip(by: -1) }
            return .success
        }
        #endif
    }

    /// Removes the lock-screen handlers; the reader calls it when it closes.
    func removeRemoteCommands() {
        #if os(iOS)
        guard remoteCommandsAdded else { return }
        remoteCommandsAdded = false
        let center = MPRemoteCommandCenter.shared()
        for command in [center.playCommand, center.pauseCommand, center.togglePlayPauseCommand,
                        center.nextTrackCommand, center.previousTrackCommand] {
            command.removeTarget(nil)
        }
        #endif
    }

    private func updateNowPlaying() {
        #if os(iOS)
        guard state != .stopped else { return }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: chapterTitle,
            MPMediaItemPropertyArtist: author,
            MPMediaItemPropertyAlbumTitle: bookTitle,
            MPNowPlayingInfoPropertyPlaybackRate: state == .playing ? 1.0 : 0.0,
        ]
        if let coverFile, let image = UIImage(contentsOfFile: coverFile) {
            info[MPMediaItemPropertyArtwork] = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
        #endif
    }

    private func clearNowPlaying() {
        #if os(iOS)
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        #endif
    }
}

extension ReadAloud: AVSpeechSynthesizerDelegate {
    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        let id = ObjectIdentifier(utterance)
        Task { @MainActor in self.finished(id) }
    }

    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        willSpeakRangeOfSpeechString characterRange: NSRange,
        utterance: AVSpeechUtterance
    ) {
        let id = ObjectIdentifier(utterance)
        Task { @MainActor in self.willSpeak(characterRange, in: id) }
    }
}
