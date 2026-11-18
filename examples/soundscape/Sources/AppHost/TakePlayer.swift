#if os(iOS)
@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import AVFoundation
import MediaPlayer
import UIKit
import Observation

private let log = ForgeLog("Listen")

/// Plays takes streamed from the take server: a queue of songs one after
/// another, or two takes side by side to compare. The queue's rules (what
/// plays next, shuffle, repeat) are Rust's (`rust/src/queue.rs`). In a
/// comparison both takes play together, in step, and switching only
/// changes which one you hear, so A and B are heard at the same moment of
/// the song. Volumes come from Rust's loudness match.
///
/// Keeps playing with the screen locked (`background_modes = ["audio"]`)
/// and shows on the lock screen with play, pause and seeking.
@Observable
@MainActor
final class TakePlayer {
    enum Side: Equatable { case a, b }

    /// What one player has loaded: a recording, with what to show for it.
    struct Loaded: Equatable {
        /// The track's title.
        let song: String
        let artist: String?
        let album: String?
        let artworkUrl: String?
        /// The recording's id and label ("seed 7202 · xl-turbo").
        let id: String
        let label: String
        let audioUrl: String
        /// The library track it plays for; nil in a comparison.
        let trackId: String?
        /// The words this recording sings, with section tags.
        let lyrics: String?

        init(_ track: QueueTrack) {
            trackId = track.trackId
            lyrics = track.recording.lyrics
            song = track.title
            artist = track.artist
            album = track.album
            artworkUrl = track.artworkUrl
            id = track.recording.id
            label = track.recording.label
            audioUrl = track.recording.audioUrl
        }

        init(song: String, take: Take) {
            trackId = nil
            lyrics = take.lyrics
            self.song = song
            artist = nil
            album = nil
            artworkUrl = nil
            id = take.id
            label = take.label
            audioUrl = take.audioUrl
        }
    }

    /// What is loaded: one take, or two when comparing.
    private(set) var a: Loaded?
    private(set) var b: Loaded?
    /// Which side you hear while comparing.
    private(set) var active: Side = .a
    private(set) var match: LoudnessMatch?
    /// What plays one after another, when not comparing.
    private(set) var queue: PlayQueue?
    private(set) var isPlaying = false
    private(set) var isBuffering = false
    private(set) var time: Double = 0
    private(set) var duration: Double = 0
    /// Set when a take couldn't be streamed.
    var failure: String?
    /// Called once per play of a queued song, once it has played long
    /// enough to count (Rust's `countsAsPlayed`), for the listening history.
    @ObservationIgnored var onPlayed: ((_ trackId: String, _ recordingId: String) -> Void)?
    @ObservationIgnored private var counted = false

    var isComparing: Bool { b != nil }
    /// Where the heard take is now, read from the player itself (`time`
    /// moves four times a second), for lyrics that follow word by word.
    var exactTime: Double {
        let seconds = (active == .b && isComparing ? playerB : playerA).currentTime().seconds
        return seconds.isFinite ? seconds : time
    }
    var current: Loaded? { active == .b ? b ?? a : a }
    /// The songs after this one, for Up Next.
    var upcoming: [QueueEntry] { queue.map { queueUpcoming(queue: $0) } ?? [] }
    var shuffle: Bool { queue?.shuffle ?? false }
    var repeatMode: RepeatMode { queue?.repeat ?? .off }
    var hasPrevious: Bool { queue != nil }
    var hasNext: Bool {
        guard let queue else { return false }
        return queue.repeat != .off || Int(queue.position) + 1 < queue.order.count
    }

    @ObservationIgnored private let playerA = AVPlayer()
    @ObservationIgnored private let playerB = AVPlayer()
    @ObservationIgnored private var timeObserver: Any?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    @ObservationIgnored private var remoteReady = false
    /// The lock screen's picture for `current`, and the address it came from.
    @ObservationIgnored private var artwork: MPMediaItemArtwork?
    @ObservationIgnored private var artworkUrl: String?
    @ObservationIgnored private var artworkTask: Task<Void, Never>?
    /// Watches the system: interruptions, unplugged headphones, and each
    /// player stopping or starting by itself.
    @ObservationIgnored private var systemObservers: [NSObjectProtocol] = []
    @ObservationIgnored private var statusObservers: [NSKeyValueObservation] = []
    /// Playing when an interruption began, so it may carry on after.
    @ObservationIgnored private var interruptedWhilePlaying = false

    init() {
        for player in [playerA, playerB] {
            player.automaticallyWaitsToMinimizeStalling = true
            player.actionAtItemEnd = .pause
        }
        timeObserver = playerA.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.25, preferredTimescale: 600), queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        watchSystem()
    }

    // MARK: Loading

    /// Plays `tracks` from `start`, one after another. Repeat carries over
    /// from what played before.
    func play(_ tracks: [QueueTrack], from start: Int = 0, shuffle: Bool = false) {
        guard !tracks.isEmpty else { return }
        queue = queueStart(
            tracks: tracks, start: UInt32(max(0, start)), shuffle: shuffle,
            repeat: repeatMode, seed: Self.seed()
        )
        loadQueued(playing: true)
    }

    /// Adds songs right after the one playing (`next`) or at the end; with
    /// nothing playing, they start.
    func add(_ tracks: [QueueTrack], next: Bool) {
        guard !tracks.isEmpty else { return }
        guard let queue, !isComparing else {
            play(tracks)
            return
        }
        self.queue = queueAdd(queue: queue, tracks: tracks, next: next)
    }

    /// Loads two takes to compare, A heard first, both from the start.
    func compare(_ first: Take, _ second: Take, of song: String, match: LoudnessMatch) {
        queue = nil
        load(
            a: Loaded(song: song, take: first),
            b: Loaded(song: song, take: second), match: match
        )
        resume()
    }

    func stop() {
        playerA.pause()
        playerB.pause()
        playerA.replaceCurrentItem(with: nil)
        playerB.replaceCurrentItem(with: nil)
        a = nil
        b = nil
        match = nil
        queue = nil
        isPlaying = false
        time = 0
        duration = 0
        artworkTask?.cancel()
        artwork = nil
        artworkUrl = nil
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        ForgeAudioSession.deactivate()
    }

    private func load(a: Loaded, b: Loaded?, match: LoudnessMatch?) {
        failure = nil
        self.a = a
        self.b = b
        self.match = match
        active = .a
        time = 0
        duration = 0
        counted = false
        playerA.replaceCurrentItem(with: Self.item(a.audioUrl))
        playerB.replaceCurrentItem(with: b.flatMap { Self.item($0.audioUrl) })
        applyVolumes()
        observeEnds()
        setUpRemoteCommands()
        loadArtwork(a.artworkUrl)
    }

    private static func item(_ audioUrl: String) -> AVPlayerItem? {
        guard let url = URL(string: audioUrl) else { return nil }
        let item = AVPlayerItem(url: url)
        item.preferredForwardBufferDuration = 30
        return item
    }

    /// Loads the queue's current song from its start.
    private func loadQueued(playing: Bool) {
        guard let queue, let track = queueCurrent(queue: queue) else { return }
        load(a: Loaded(track), b: nil, match: nil)
        if playing { resume() } else { pause() }
    }

    // MARK: The queue

    /// The next song; at the end of the queue with repeat off, back to the
    /// first song, paused.
    func next() {
        guard let queue, !isComparing else { return }
        if let moved = queueAdvance(queue: queue, byUser: true) {
            self.queue = moved
            loadQueued(playing: true)
        } else {
            self.queue = queueJump(queue: queue, position: 0)
            loadQueued(playing: false)
        }
    }

    /// Back to the start of the song, or, near its start, the song before.
    func previous() {
        guard let queue, !isComparing else { return }
        if time > 3 {
            seek(to: 0)
            return
        }
        self.queue = queueBack(queue: queue)
        loadQueued(playing: isPlaying)
    }

    /// Plays the song at `position` in the play order (from Up Next).
    func jump(to position: UInt32) {
        guard let queue else { return }
        self.queue = queueJump(queue: queue, position: position)
        loadQueued(playing: true)
    }

    func remove(at position: UInt32) {
        guard let queue else { return }
        self.queue = queueRemove(queue: queue, position: position)
    }

    func toggleShuffle() {
        guard let queue else { return }
        self.queue = queueSetShuffle(queue: queue, on: !queue.shuffle, seed: Self.seed())
    }

    /// Off, All, One, Off.
    func cycleRepeat() {
        guard let queue else { return }
        self.queue = queueSetRepeat(queue: queue, repeat: repeatAfter(repeat: queue.repeat))
    }

    private static func seed() -> UInt64 { UInt64.random(in: .min ... .max) }

    // MARK: Transport

    func toggle() {
        isPlaying ? pause() : resume()
    }

    func resume() {
        guard a != nil else { return }
        do {
            // Music, not speech: the "playback" category with default mode,
            // so it plays with the silent switch on and over other apps.
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .default)
            try session.setActive(true)
        } catch {
            log.warn("audio session: \(error)")
        }
        if isComparing { resync() }
        playerA.play()
        if isComparing { playerB.play() }
        isPlaying = true
        updateNowPlaying()
    }

    func pause() {
        playerA.pause()
        playerB.pause()
        isPlaying = false
        updateNowPlaying()
    }

    func seek(to seconds: Double) {
        let target = CMTime(seconds: max(0, seconds), preferredTimescale: 600)
        playerA.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero)
        if isComparing {
            playerB.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero)
        }
        time = seconds
        updateNowPlaying()
    }

    func skip(_ seconds: Double) {
        seek(to: min(max(0, time + seconds), duration > 0 ? duration : time + seconds))
    }

    /// Switches which take you hear in a comparison, at the same moment.
    func hear(_ side: Side) {
        guard isComparing, side != active else { return }
        active = side
        resync()
        applyVolumes()
        updateNowPlaying()
    }

    // MARK: Inside

    private func applyVolumes() {
        guard isComparing else {
            playerA.volume = 1
            return
        }
        let m = match ?? loudnessMatch(aLufs: nil, bLufs: nil)
        playerA.volume = active == .a ? m.aVolume : 0
        playerB.volume = active == .b ? m.bVolume : 0
    }

    /// Streaming can stall one player and not the other; before switching,
    /// line the one about to be heard up with the one heard now.
    private func resync() {
        let target = active == .a ? playerA : playerB
        let reference = active == .a ? playerB : playerA
        guard reference.currentItem != nil, target.currentItem != nil else { return }
        let now = reference.currentTime()
        guard now.isValid, now.seconds.isFinite else { return }
        if abs(target.currentTime().seconds - now.seconds) > 0.15 {
            target.seek(to: now, toleranceBefore: .zero, toleranceAfter: .zero)
        }
    }

    private func tick() {
        let heard = active == .b && isComparing ? playerB : playerA
        let seconds = heard.currentTime().seconds
        if seconds.isFinite { time = seconds }
        if let d = heard.currentItem?.duration.seconds, d.isFinite, d > 0, d != duration {
            // The length is known only once the stream has started, after
            // the lock screen was first told about the song.
            duration = d
            updateNowPlaying()
        }
        isBuffering = heard.timeControlStatus == .waitingToPlayAtSpecifiedRate
        if let item = heard.currentItem, item.status == .failed, failure == nil {
            failure = item.error?.localizedDescription ?? "The take couldn't be played."
            log.warn("stream failed: \(failure ?? "")")
            pause()
        }
        checkStoppedBySystem()
        countPlay()
    }

    private func countPlay() {
        guard !counted, isPlaying, !isComparing, let current, let trackId = current.trackId,
              countsAsPlayed(listened: time, length: duration > 0 ? duration : nil) else { return }
        counted = true
        onPlayed?(trackId, current.id)
    }

    // MARK: The system

    /// Another app's sound, a call, Siri or unplugged headphones can stop
    /// the players without asking. Each way it can happen is watched, so
    /// the buttons, the mini-player and the lock screen show Play as soon
    /// as the sound stops.
    private func watchSystem() {
        let center = NotificationCenter.default
        systemObservers.append(center.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            let info = note.userInfo
            let type = (info?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init(rawValue:))
            let options = AVAudioSession.InterruptionOptions(
                rawValue: info?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
            )
            MainActor.assumeIsolated {
                guard let self else { return }
                switch type {
                case .began:
                    self.interruptedWhilePlaying = self.isPlaying
                    self.stoppedBySystem()
                case .ended:
                    // A call or Siri hands the sound back; another app that
                    // started its own music doesn't.
                    if self.interruptedWhilePlaying, options.contains(.shouldResume) { self.resume() }
                    self.interruptedWhilePlaying = false
                default:
                    break
                }
            }
        })
        systemObservers.append(center.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
        ) { [weak self] note in
            let reason = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
            guard reason == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue else { return }
            // Headphones unplugged: iOS pauses, and so do we, both sides.
            MainActor.assumeIsolated { self?.stoppedBySystem() }
        })
        for player in [playerA, playerB] {
            statusObservers.append(player.observe(\.timeControlStatus) { [weak self] _, _ in
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { self?.checkStoppedBySystem() }
                }
            })
        }
    }

    /// The player you hear stopped, and not at the end of the song (that
    /// is `ended`): the system paused it.
    private func checkStoppedBySystem() {
        guard isPlaying else { return }
        let heard = active == .b && isComparing ? playerB : playerA
        guard heard.timeControlStatus == .paused, let item = heard.currentItem,
              item.status == .readyToPlay else { return }
        let at = heard.currentTime().seconds
        let length = item.duration.seconds
        if length.isFinite, length > 0, at >= length - 0.5 { return }
        stoppedBySystem()
    }

    private func stoppedBySystem() {
        guard isPlaying else { return }
        // In a comparison the other side keeps in step only if it stops too.
        playerA.pause()
        playerB.pause()
        isPlaying = false
        let seconds = (active == .b && isComparing ? playerB : playerA).currentTime().seconds
        if seconds.isFinite { time = seconds }
        updateNowPlaying()
    }

    private func observeEnds() {
        for o in observers { NotificationCenter.default.removeObserver(o) }
        observers = []
        for side in [Side.a, Side.b] {
            guard let item = (side == .a ? playerA : playerB).currentItem else { continue }
            observers.append(NotificationCenter.default.addObserver(
                forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.ended(side) }
            })
        }
    }

    /// The take you hear came to its end: the queue's next song, the same
    /// song again (repeat One), or back to the start, paused.
    private func ended(_ side: Side) {
        let heard: Side = isComparing ? active : .a
        guard side == heard else { return }
        if !isComparing, let queue {
            if let moved = queueAdvance(queue: queue, byUser: false) {
                if moved.position == queue.position && moved.order.count == queue.order.count {
                    counted = false
                    seek(to: 0)
                    resume()
                } else {
                    self.queue = moved
                    loadQueued(playing: true)
                }
                return
            }
        }
        pause()
        seek(to: 0)
    }

    // MARK: Lock screen

    private func updateNowPlaying() {
        guard let current else { return }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: isComparing
                ? "\(active == .a ? "A" : "B"): \(current.label)"
                : current.song,
            MPMediaItemPropertyAlbumTitle: isComparing ? current.song : current.album ?? current.label,
            MPMediaItemPropertyArtist: isComparing ? "Comparing takes" : current.artist ?? current.label,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: time,
            MPNowPlayingInfoPropertyPlaybackRate: isPlaying ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if duration > 0 { info[MPMediaItemPropertyPlaybackDuration] = duration }
        if let artwork, !isComparing { info[MPMediaItemPropertyArtwork] = artwork }
        if let queue, !isComparing {
            info[MPNowPlayingInfoPropertyPlaybackQueueIndex] = Int(queue.position)
            info[MPNowPlayingInfoPropertyPlaybackQueueCount] = queue.order.count
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    /// Fetches the song's artwork for the lock screen. The song shows
    /// without it until it arrives; a song with none shows none.
    private func loadArtwork(_ url: String?) {
        guard url != artworkUrl else { return }
        artworkTask?.cancel()
        artworkUrl = url
        artwork = nil
        guard let url, let address = URL(string: url) else { return }
        artworkTask = Task { [weak self] in
            // Through the same cache as the library's pictures, so it is
            // usually already there. A picture chosen on the phone is a file.
            let picture = address.isFileURL
                ? await ForgeImages.thumbnail(of: address, maxPixels: 1024, cached: true)
                : await ForgeImages.thumbnail(from: address, maxPixels: 1024)
            guard let picture, !Task.isCancelled else { return }
            guard let self, self.artworkUrl == url else { return }
            self.artwork = Self.lockScreenArtwork(UIImage(cgImage: picture))
            self.updateNowPlaying()
        }
    }

    /// Made outside the main actor: MediaPlayer asks for the picture on a
    /// background queue, and a closure made on the main actor would trap
    /// there.
    nonisolated private static func lockScreenArtwork(_ image: UIImage) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: image.size) { _ in image }
    }

    private func setUpRemoteCommands() {
        guard !remoteReady else { return }
        remoteReady = true
        let center = MPRemoteCommandCenter.shared()
        center.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.resume() }
            return .success
        }
        center.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.pause() }
            return .success
        }
        center.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.toggle() }
            return .success
        }
        center.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let position = event.positionTime
            MainActor.assumeIsolated { self?.seek(to: position) }
            return .success
        }
        center.skipBackwardCommand.preferredIntervals = [10]
        center.skipBackwardCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.skip(-10) }
            return .success
        }
        // In a comparison, "next" and "previous" on headphones or the lock
        // screen switch between A and B; otherwise they move through the
        // queue.
        center.nextTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.isComparing { self.hear(self.active == .a ? .b : .a) } else { self.next() }
            }
            return .success
        }
        center.previousTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.isComparing { self.hear(self.active == .a ? .b : .a) } else { self.previous() }
            }
            return .success
        }
    }
}
#endif
