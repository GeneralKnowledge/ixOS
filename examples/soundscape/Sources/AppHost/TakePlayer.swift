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
/// Songs in a queue run into each other with no gap, or overlap by
/// `crossfade` seconds: the next song is loaded on the second player
/// before this one ends and takes over from it, and the two players swap
/// roles.
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
    /// What plays one after another, when not comparing. Kept (`onSave`)
    /// whenever it changes, so the app opens where it was left.
    private(set) var queue: PlayQueue? {
        didSet {
            save()
            // The next song was loaded for the queue as it was.
            if let prepared, queue != prepared.from { cancelPrepared() }
        }
    }
    private(set) var isPlaying = false
    private(set) var isBuffering = false
    private(set) var time: Double = 0
    private(set) var duration: Double = 0
    /// Set when a take couldn't be streamed.
    var failure: String?
    /// Called once per play of a queued song, once it has played long
    /// enough to count (Rust's `countsAsPlayed`), for the listening history.
    @ObservationIgnored var onPlayed: ((_ trackId: String, _ recordingId: String) -> Void)?
    /// Called with what is playing whenever it changes, now and then while
    /// it plays, and with `nil` when playback stops, to keep it across
    /// restarts (Rust's `save_playback`).
    @ObservationIgnored var onSave: ((SavedPlayback?) -> Void)?
    @ObservationIgnored private var lastSave = Date.distantPast
    /// The sleep timer: playback fades out over its last ten seconds and
    /// pauses at this moment.
    private(set) var sleepAt: Date?
    /// The sleep timer set to the end of the song: the next one waits.
    private(set) var sleepAtEndOfSong = false
    /// How long the fade before a sleep timer's pause takes.
    static let sleepFade: TimeInterval = 10
    @ObservationIgnored private var counted = false
    /// Seconds the end of one song overlaps the start of the next; 0 plays
    /// them back to back with no gap. Set from Rust's saved setting.
    var crossfade: Double = 0 {
        didSet { cancelPrepared() }
    }
    /// How long before a song ends the next is loaded.
    static let prepareAhead: TimeInterval = 30
    /// How long before a song ends the next is started at the exact moment
    /// it ends (no crossfade).
    static let gaplessLead: TimeInterval = 0.5

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

    /// Two players. In a comparison A is one take and B the other; in a
    /// queue A plays the song and B holds the next, then they swap.
    @ObservationIgnored private let players = [AVPlayer(), AVPlayer()]
    @ObservationIgnored private var mainIndex = 0
    private var playerA: AVPlayer { players[mainIndex] }
    private var playerB: AVPlayer { players[1 - mainIndex] }
    @ObservationIgnored private var timeObservers: [Any] = []
    /// The next song, loaded on B before A's song ends.
    private struct Prepared {
        /// The queue it was loaded for, and the queue once it plays.
        let from: PlayQueue
        let to: PlayQueue
        let loaded: Loaded
    }
    @ObservationIgnored private var prepared: Prepared?
    /// Where A starts the next song: `crossfade` (or `gaplessLead`) before
    /// its end.
    @ObservationIgnored private var boundary: (player: AVPlayer, token: Any)?
    /// While the song that ended is still heard on B after the next took
    /// over: when that began and how long its fade lasts (0: it plays out
    /// its last moment at full volume).
    @ObservationIgnored private var handoverStart: Date?
    @ObservationIgnored private var fadeLength: TimeInterval = 0
    @ObservationIgnored private var fadeTimer: Timer?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    @ObservationIgnored private let remoteTargets = RemoteCommandTargets()
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
        for (index, player) in players.enumerated() {
            player.automaticallyWaitsToMinimizeStalling = true
            player.actionAtItemEnd = .pause
            // Either player can be A; only A's clock moves the screen.
            timeObservers.append(player.addPeriodicTimeObserver(
                forInterval: CMTime(seconds: 0.25, preferredTimescale: 600), queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, index == self.mainIndex else { return }
                    self.tick()
                }
            })
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
        load(
            a: Loaded(song: song, take: first),
            b: Loaded(song: song, take: second), match: match
        )
        // A comparison isn't kept: the saved queue stays what it was.
        queue = nil
        resume()
    }

    func stop() {
        endHandover()
        cancelPrepared()
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
        remoteTargets.removeAll()
        ForgeAudioSession.deactivate()
    }

    private func load(a: Loaded, b: Loaded?, match: LoudnessMatch?) {
        endHandover()
        cancelPrepared()
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
        endHandover()
        playerA.pause()
        if isComparing { playerB.pause() }
        isPlaying = false
        updateNowPlaying()
        save()
    }

    // MARK: Sleep timer

    /// Pauses after `minutes`, fading out over the last ten seconds.
    func sleep(after minutes: Int) {
        sleepAtEndOfSong = false
        sleepAt = Date().addingTimeInterval(TimeInterval(minutes * 60))
    }

    /// Pauses when the playing song ends; the next one is ready to play.
    func sleepAtEndOfThisSong() {
        sleepAt = nil
        sleepAtEndOfSong = true
    }

    func cancelSleep() {
        sleepAt = nil
        sleepAtEndOfSong = false
        if handoverStart == nil { applyVolumes() } else { mixVolumes() }
    }

    /// Fades, then pauses, as the sleep timer runs out.
    private func checkSleep() {
        guard let sleepAt, !isComparing else { return }
        let left = sleepAt.timeIntervalSinceNow
        if left <= 0 {
            self.sleepAt = nil
            pause()
            applyVolumes()
            log.info("sleep timer paused playback")
        } else if left < Self.sleepFade, isPlaying {
            mixVolumes()
        }
    }

    // MARK: Keeping what plays

    /// Hands what is playing to `onSave`: the queue and how far into the
    /// song. Nothing while comparing; `nil` once stopped.
    func save() {
        guard !isComparing else { return }
        lastSave = Date()
        guard let queue else {
            onSave?(nil)
            return
        }
        onSave?(SavedPlayback(queue: queue, seconds: time, savedMs: Int64(Date().timeIntervalSince1970 * 1000)))
    }

    /// Opens what was playing when the app closed, paused at the same
    /// moment, ready to carry on with Play.
    func restore(_ saved: SavedPlayback) {
        guard current == nil else { return }
        queue = saved.queue
        loadQueued(playing: false)
        if saved.seconds > 0 { seek(to: saved.seconds) }
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
        if isPlaying, Date().timeIntervalSince(lastSave) > 5 { save() }
        checkSleep()
        prepareNext()
        if let item = heard.currentItem, item.status == .failed, failure == nil {
            failure = Self.describe(item.error, playing: current)
            log.warn("stream failed: \(item.error.map { "\($0)" } ?? "")")
            pause()
        }
        checkStoppedBySystem()
        countPlay()
    }

    /// Why a recording can't play, in words. A catalogue that lists a
    /// track but serves no file for it (Jamendo answers 404 for some) is
    /// said plainly rather than as a network failure.
    static func describe(_ error: Error?, playing: Loaded?) -> String {
        let nsError = error as NSError?
        if let id = playing?.id, id.hasPrefix("jamendo:"), nsError?.domain != NSURLErrorDomain {
            return "Jamendo has no playable file for this track right now."
        }
        return nsError?.localizedDescription ?? "The take couldn't be played."
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
        // A song started at the moment the one before ends waits until then.
        if let handoverStart, Date().timeIntervalSince(handoverStart) < Self.gaplessLead + 1 { return }
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
        endHandover()
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
        if side == .b, !isComparing {
            // The song that handed over has finished.
            endHandover()
            return
        }
        let heard: Side = isComparing ? active : .a
        guard side == heard else { return }
        if !isComparing, let queue {
            if !sleepAtEndOfSong, let prepared, prepared.from == queue,
               playerB.currentItem?.status == .readyToPlay {
                // Ended before the hand-over was due (a seek past it): the
                // next song, already loaded, starts now.
                handOver(prepared, now: true)
                return
            }
            if sleepAtEndOfSong {
                // The sleep timer ends with this song: the next waits, paused.
                sleepAtEndOfSong = false
                if let moved = queueAdvance(queue: queue, byUser: true) {
                    self.queue = moved
                    loadQueued(playing: false)
                } else {
                    pause()
                }
                return
            }
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

    // MARK: Between songs

    /// Near the end of a queued song, loads the next on B, quietly, and
    /// marks where A hands over to it.
    private func prepareNext() {
        guard prepared == nil, handoverStart == nil, isPlaying, !isComparing, !sleepAtEndOfSong,
              let queue, duration > 0, duration - time < Self.prepareAhead + crossfade,
              let moved = queueAdvance(queue: queue, byUser: false),
              // Repeat One plays the same song again from its start.
              moved.position != queue.position || moved.order.count != queue.order.count,
              let track = queueCurrent(queue: moved) else { return }
        let next = Loaded(track)
        guard let item = Self.item(next.audioUrl) else { return }
        playerB.volume = 0
        playerB.replaceCurrentItem(with: item)
        prepared = Prepared(from: queue, to: moved, loaded: next)
        let at = max(time, duration - max(crossfade, Self.gaplessLead))
        let main = playerA
        let token = main.addBoundaryTimeObserver(
            forTimes: [NSValue(time: CMTime(seconds: at, preferredTimescale: 600))], queue: .main
        ) { [weak self] in
            MainActor.assumeIsolated { self?.startPrepared() }
        }
        boundary = (main, token)
    }

    /// A reached the hand-over point: the next song starts if it is ready;
    /// otherwise A plays to its end and the next loads then, as before.
    private func startPrepared() {
        clearBoundary()
        guard let prepared, prepared.from == queue, isPlaying, !isComparing, !sleepAtEndOfSong,
              playerB.currentItem?.status == .readyToPlay else { return }
        handOver(prepared, now: false)
    }

    /// The next song, loaded on B, becomes the song playing, and the
    /// players swap: the one ending, now B, fades out (crossfade) or plays
    /// its last moment (gapless) while the next starts on A.
    private func handOver(_ prepared: Prepared, now: Bool) {
        let ending = playerA
        let next = playerB
        if now {
            next.volume = 1
            next.play()
            fadeLength = 0
        } else if crossfade > 0 {
            next.volume = 0
            next.play()
            fadeLength = crossfade
        } else {
            // Starts the moment A's song ends, by the host clock, so there
            // is no gap.
            let left = (ending.currentItem?.duration.seconds ?? 0) - ending.currentTime().seconds
            let wait = CMTime(seconds: left.isFinite ? max(0, left) : 0, preferredTimescale: 1_000_000_000)
            next.volume = 1
            next.automaticallyWaitsToMinimizeStalling = false
            next.setRate(1, time: .zero, atHostTime: CMTimeAdd(CMClockGetTime(CMClockGetHostTimeClock()), wait))
            fadeLength = 0
        }
        self.prepared = nil
        mainIndex = 1 - mainIndex
        a = prepared.loaded
        queue = prepared.to
        time = 0
        duration = 0
        counted = false
        failure = nil
        observeEnds()
        loadArtwork(prepared.loaded.artworkUrl)
        updateNowPlaying()
        if now {
            ending.pause()
            ending.replaceCurrentItem(with: nil)
            return
        }
        handoverStart = Date()
        if fadeLength > 0 {
            fadeTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.mixVolumes() }
            }
        }
        log.info(fadeLength > 0 ? "crossfade to the next song" : "next song starts with no gap")
    }

    /// The volumes outside a comparison: the crossfade between the song
    /// ending (B) and the next (A), under the sleep timer's fade.
    private func mixVolumes() {
        guard !isComparing else { return }
        var sleep: Float = 1
        if let sleepAt {
            sleep = Float(min(1, max(0, sleepAt.timeIntervalSinceNow / Self.sleepFade)))
        }
        guard let handoverStart else {
            playerA.volume = sleep
            return
        }
        guard fadeLength > 0 else {
            playerA.volume = sleep
            playerB.volume = sleep
            return
        }
        let done = Float(min(1, Date().timeIntervalSince(handoverStart) / fadeLength))
        playerA.volume = done * sleep
        playerB.volume = (1 - done) * sleep
        if done >= 1 { endHandover() }
    }

    /// The song that handed over is done: B is cleared for the next one.
    private func endHandover() {
        fadeTimer?.invalidate()
        fadeTimer = nil
        guard handoverStart != nil else { return }
        handoverStart = nil
        fadeLength = 0
        playerB.pause()
        playerB.replaceCurrentItem(with: nil)
        playerA.automaticallyWaitsToMinimizeStalling = true
        mixVolumes()
        observeEnds()
    }

    /// Forgets the next song loaded on B, when the queue changed under it.
    private func cancelPrepared() {
        clearBoundary()
        guard prepared != nil else { return }
        prepared = nil
        playerB.replaceCurrentItem(with: nil)
    }

    private func clearBoundary() {
        if let boundary { boundary.player.removeTimeObserver(boundary.token) }
        boundary = nil
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

    /// Adds the player's lock-screen handlers on the first song after a
    /// stop; `stop()` removes them again, and only them.
    private func setUpRemoteCommands() {
        guard remoteTargets.isEmpty else { return }
        let center = MPRemoteCommandCenter.shared()
        remoteTargets.add(center.playCommand) { [weak self] _ in
            MainActor.assumeIsolated { self?.resume() }
            return .success
        }
        remoteTargets.add(center.pauseCommand) { [weak self] _ in
            MainActor.assumeIsolated { self?.pause() }
            return .success
        }
        remoteTargets.add(center.togglePlayPauseCommand) { [weak self] _ in
            MainActor.assumeIsolated { self?.toggle() }
            return .success
        }
        remoteTargets.add(center.changePlaybackPositionCommand) { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let position = event.positionTime
            MainActor.assumeIsolated { self?.seek(to: position) }
            return .success
        }
        center.skipBackwardCommand.preferredIntervals = [10]
        remoteTargets.add(center.skipBackwardCommand) { [weak self] _ in
            MainActor.assumeIsolated { self?.skip(-10) }
            return .success
        }
        // In a comparison, "next" and "previous" on headphones or the lock
        // screen switch between A and B; otherwise they move through the
        // queue.
        remoteTargets.add(center.nextTrackCommand) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.isComparing { self.hear(self.active == .a ? .b : .a) } else { self.next() }
            }
            return .success
        }
        remoteTargets.add(center.previousTrackCommand) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.isComparing { self.hear(self.active == .a ? .b : .a) } else { self.previous() }
            }
            return .success
        }
    }
}

/// The lock-screen handlers one player added, so it removes only its own:
/// the command center is shared by every app in Device Lab, and
/// `removeTarget(nil)` would take away another app's controls too. Removes
/// them when it goes away as well, so a player that is freed without
/// stopping leaves no dead buttons behind (Soundscape's did, found on the
/// phone, 2026-11-18). Each app keeps its own copy until the Now Playing
/// helper is extracted.
private final class RemoteCommandTargets {
    private var targets: [(command: MPRemoteCommand, token: Any)] = []

    var isEmpty: Bool { targets.isEmpty }

    func add(
        _ command: MPRemoteCommand,
        _ handler: @escaping @Sendable (MPRemoteCommandEvent) -> MPRemoteCommandHandlerStatus
    ) {
        targets.append((command, command.addTarget(handler: handler)))
    }

    func removeAll() {
        for target in targets { target.command.removeTarget(target.token) }
        targets = []
    }

    deinit { removeAll() }
}
#endif
