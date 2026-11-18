@preconcurrency import AVFoundation
@preconcurrency import PocketLibraryFFI
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
/// forge.toml) and answers the lock screen's play, pause, skip back and
/// forward, and progress bar.
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

    /// A voice identifier, or `nil` for the best installed voice of the
    /// device's language (see `bestVoice()`). Settings writes the same key.
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
    @ObservationIgnored private var cachedVoice: (identifier: String, voice: AVSpeechSynthesisVoice?)?

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
        texts = chapter.section.blocks.map(Self.speakableText)
        chapterTitle = chapter.section.title
        word = nil
        block = nil
        utterance = nil
        // Picks up voices downloaded while the reader was open.
        cachedVoice = nil
        if synthesizer.isSpeaking { synthesizer.stopSpeaking(at: .immediate) }
        updateNowPlaying()
    }

    /// The text spoken for a block: exactly the text the reader shows, so
    /// word ranges line up with it. Scene breaks and pictures are skipped.
    nonisolated static func speakableText(_ block: ReaderBlock) -> String {
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

    /// How far the lock screen's skip buttons move.
    nonisolated static let skipInterval: Double = 10

    /// Moves about `seconds` back or forward in the chapter (the lock
    /// screen's skip buttons). Speech can only start at a block, so this
    /// goes to the block holding the new time: back restarts the current
    /// block when the time is still inside it, and forward always moves
    /// on at least one block, so it never goes backwards.
    func skip(seconds: Double) {
        guard let current = block else { return }
        let now = Self.estimatedTimes(texts: texts, block: current, speed: speed, within: spokenOffset).elapsed
        guard var target = Self.block(at: now + seconds, texts: texts, speed: speed) else { return }
        if seconds > 0, target <= current {
            target = current + 1
            while target < texts.count, texts[target].isEmpty { target += 1 }
            // Past the last block: let the chapter end as it would.
        }
        jump(to: target)
    }

    /// Moves to the block at `seconds` into the chapter (the lock screen's
    /// progress bar), by the same estimate that gives the lock screen its
    /// duration.
    func seek(toSeconds seconds: Double) {
        guard let target = Self.block(at: seconds, texts: texts, speed: speed) else { return }
        jump(to: target)
    }

    /// Reads on from `index` when playing; when paused, moves there and
    /// stays paused, so play carries on from the new place.
    private func jump(to index: Int) {
        guard state != .stopped else { return }
        if state == .playing {
            activateAudioSession()
            speak(from: index)
            return
        }
        var index = max(index, 0)
        while index < texts.count, texts[index].isEmpty { index += 1 }
        guard index < texts.count else { return }
        utterance = nil
        if synthesizer.isSpeaking || synthesizer.isPaused {
            synthesizer.stopSpeaking(at: .immediate)
        }
        block = index
        word = nil
        onBlock?(index)
        updateNowPlaying()
    }

    /// How far into the current block the voice is, in characters.
    private var spokenOffset: Int {
        guard let block, block < texts.count, let word else { return 0 }
        return min(word.location, texts[block].count)
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
        ForgeAudioSession.deactivate()
    }

    // MARK: Voices

    /// The installed voices, ready to choose from. Novelty voices (Bells,
    /// Bubbles…) are left out; Personal Voices appear once the user has
    /// allowed the app to use them.
    static func installedVoices() -> [VoiceCandidate] {
        AVSpeechSynthesisVoice.speechVoices()
            .filter { !$0.voiceTraits.contains(.isNoveltyVoice) }
            .map { voice in
                let quality: VoiceCandidate.Quality = switch voice.quality {
                case .premium: .premium
                case .enhanced: .enhanced
                default: .standard
                }
                return VoiceCandidate(
                    id: voice.identifier,
                    name: voice.name,
                    language: voice.language,
                    quality: quality,
                    isPersonal: voice.voiceTraits.contains(.isPersonalVoice)
                )
            }
    }

    /// The device's language, such as "en-GB".
    static var language: String { AVSpeechSynthesisVoice.currentLanguageCode() }

    /// The voices to offer, best first. Better voices can be downloaded in
    /// Settings > Accessibility > Spoken Content > Voices.
    static func voices() -> [VoiceCandidate] {
        VoiceChoice.ranked(installedVoices(), language: language)
    }

    /// The voice used for "Best Available": iOS's own default for a
    /// language is the small standard voice even when an Enhanced or
    /// Premium one is installed, so pick by quality.
    static func bestVoice() -> AVSpeechSynthesisVoice? {
        VoiceChoice.best(installedVoices(), language: language)
            .flatMap { AVSpeechSynthesisVoice(identifier: $0.id) }
            ?? AVSpeechSynthesisVoice(language: language)
    }

    /// Whether the user can be asked for their Personal Voice: they have
    /// one (or might), and haven't answered yet.
    static var canAskForPersonalVoice: Bool {
        AVSpeechSynthesizer.personalVoiceAuthorizationStatus == .notDetermined
    }

    /// Asks to use the user's Personal Voice (made in Settings >
    /// Accessibility > Personal Voice). Returns whether it was allowed.
    static func askForPersonalVoice() async -> Bool {
        await withCheckedContinuation { continuation in
            AVSpeechSynthesizer.requestPersonalVoiceAuthorization { status in
                continuation.resume(returning: status == .authorized)
            }
        }
    }

    /// The voice to speak with: the one picked, if it is still installed,
    /// otherwise the best available. Looked up once per chapter and voice
    /// change, not per paragraph.
    private func resolvedVoice() -> AVSpeechSynthesisVoice? {
        if let cachedVoice, cachedVoice.identifier == voiceID ?? Self.bestVoiceMarker {
            return cachedVoice.voice
        }
        let voice = voiceID.flatMap(AVSpeechSynthesisVoice.init(identifier:)) ?? Self.bestVoice()
        cachedVoice = (voiceID ?? Self.bestVoiceMarker, voice)
        return voice
    }

    private static let bestVoiceMarker = ""

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
        next.voice = resolvedVoice()
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
    nonisolated static func rate(for speed: Double) -> Float {
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
    nonisolated static let voiceKey = "readAloudVoice"
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

    /// Spoken-audio playback: plays with the ring switch on silent, pauses
    /// other apps' audio, and (with the `audio` background mode) carries on
    /// with the screen locked. The lock-screen controls below stay here
    /// until a second app needs them.
    private func activateAudioSession() {
        try? ForgeAudioSession.activate(.playback)
    }

    #if os(iOS)
    /// Every lock-screen command Read Aloud answers.
    private static func remoteCommands(_ center: MPRemoteCommandCenter) -> [MPRemoteCommand] {
        [center.playCommand, center.pauseCommand, center.togglePlayPauseCommand,
         center.nextTrackCommand, center.previousTrackCommand,
         center.skipBackwardCommand, center.skipForwardCommand,
         center.changePlaybackPositionCommand]
    }
    #endif

    /// Takes over the lock-screen commands. The command center is shared by
    /// every app in Device Lab, so this first removes whatever another
    /// player left there (Soundscape's player left its skip-back button
    /// and progress bar behind, found on the phone, 2026-11-18), then adds
    /// Read Aloud's own. Done on every `play(from:)`, so reading takes the
    /// controls back after another app's player has had them.
    private func addRemoteCommands() {
        #if os(iOS)
        remoteCommandsAdded = true
        let center = MPRemoteCommandCenter.shared()
        for command in Self.remoteCommands(center) {
            command.removeTarget(nil)
            command.isEnabled = true
        }
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
        center.skipBackwardCommand.preferredIntervals = [NSNumber(value: Self.skipInterval)]
        center.skipBackwardCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in self?.skip(seconds: -Self.skipInterval) }
            return .success
        }
        center.skipForwardCommand.preferredIntervals = [NSNumber(value: Self.skipInterval)]
        center.skipForwardCommand.addTarget { @Sendable [weak self] _ in
            Task { @MainActor in self?.skip(seconds: Self.skipInterval) }
            return .success
        }
        center.changePlaybackPositionCommand.addTarget { @Sendable [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let position = event.positionTime
            Task { @MainActor in self?.seek(toSeconds: position) }
            return .success
        }
        #endif
    }

    /// Removes the lock-screen handlers; the reader calls it when it closes.
    func removeRemoteCommands() {
        #if os(iOS)
        guard remoteCommandsAdded else { return }
        remoteCommandsAdded = false
        for command in Self.remoteCommands(MPRemoteCommandCenter.shared()) {
            command.removeTarget(nil)
        }
        #endif
    }

    /// Speech has no duration of its own, so the lock screen gets an
    /// estimate from the chapter's length at the current speed: where the
    /// current block starts, and the whole chapter. iOS moves the progress
    /// bar between updates at the playback rate; each new block corrects it.
    /// Without a duration and elapsed time iOS showed the player with no
    /// details at all (found on the phone, 2026-11-17; Soundscape's player
    /// had the same fault).
    /// `within` is how many characters into the block the voice is, so a
    /// paused chapter shows where it stopped.
    nonisolated static func estimatedTimes(
        texts: [String], block: Int?, speed: Double, within: Int = 0
    ) -> (elapsed: Double, duration: Double) {
        let perSecond = charactersPerSecond(speed)
        let total = texts.reduce(0) { $0 + $1.count }
        let before = texts.prefix(max(0, min(block ?? 0, texts.count))).reduce(0) { $0 + $1.count }
        let inside = block == nil ? 0 : max(0, within)
        return (Double(min(before + inside, total)) / perSecond, Double(total) / perSecond)
    }

    /// The block with text at `seconds` into the chapter, by the same
    /// estimate as `estimatedTimes`: the inverse the lock screen's progress
    /// bar and skip buttons need. Times before the start give the first
    /// block, times past the end the last; nil when nothing has text.
    nonisolated static func block(at seconds: Double, texts: [String], speed: Double) -> Int? {
        guard let last = texts.lastIndex(where: { !$0.isEmpty }) else { return nil }
        let position = max(0, seconds) * charactersPerSecond(speed)
        var start = 0.0
        for (index, text) in texts.enumerated() where !text.isEmpty {
            let end = start + Double(text.count)
            if position < end { return index }
            start = end
        }
        return last
    }

    /// About 15 characters a second at normal speed (≈180 words a minute).
    nonisolated private static func charactersPerSecond(_ speed: Double) -> Double {
        15 * max(speed, 0.5)
    }

    private func updateNowPlaying() {
        #if os(iOS)
        guard state != .stopped else { return }
        let times = Self.estimatedTimes(texts: texts, block: block, speed: speed, within: spokenOffset)
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: chapterTitle.isEmpty ? bookTitle : chapterTitle,
            MPMediaItemPropertyArtist: author,
            MPMediaItemPropertyAlbumTitle: bookTitle,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: times.elapsed,
            MPNowPlayingInfoPropertyPlaybackRate: state == .playing ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if times.duration > 0 { info[MPMediaItemPropertyPlaybackDuration] = times.duration }
        if let coverFile, let image = UIImage(contentsOfFile: coverFile) {
            info[MPMediaItemPropertyArtwork] = Self.lockScreenArtwork(image)
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
        #endif
    }

    #if os(iOS)
    /// Made outside the main actor: MediaPlayer asks for the picture on a
    /// background queue, and a closure made on the main actor traps there.
    nonisolated private static func lockScreenArtwork(_ image: UIImage) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: image.size) { _ in image }
    }
    #endif

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
