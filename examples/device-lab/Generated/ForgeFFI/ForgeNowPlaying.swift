// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeNowPlaying.swift.

#if os(iOS)
import AVFoundation
import MediaPlayer
import UIKit

/// A playing screen's place on the lock screen, Control Center and
/// headphones: the details shown, the buttons that work, and the system's
/// rules for calls, Siri and unplugged headphones. One per player; the
/// player itself (`AVPlayer`, `AVSpeechSynthesizer`) stays the app's.
///
///     let nowPlaying = ForgeNowPlaying(
///         isPlaying: { [weak self] in self?.isPlaying == true },
///         pause: { [weak self] in self?.pause() },
///         play: { [weak self] in self?.resume() })
///     nowPlaying.commands(.init(play: …, pause: …, next: …, seek: …))
///     nowPlaying.update(.init(title: song.title, artist: song.artist, duration: 213, position: 12, playing: true))
///     nowPlaying.end()   // when the player stops or closes
///
/// Only the buttons given handlers show. The command center is shared by
/// every app in Device Lab, so this removes only its own handlers, on
/// `end()` and when it is freed.
@MainActor
public final class ForgeNowPlaying {
    /// What the lock screen shows.
    public struct Info {
        public var title: String
        public var artist: String?
        public var album: String?
        public var artwork: UIImage?
        /// Seconds. Without a duration and a position iOS showed the player
        /// with no details at all (Read Aloud, 2026-11-17).
        public var duration: Double?
        public var position: Double?
        public var playing: Bool
        /// Where this item is in a queue, when there is one.
        public var queueIndex: Int?
        public var queueCount: Int?
        /// A live stream (a radio station): the lock screen shows it as live,
        /// with no progress bar. Give no duration or position with it.
        public var isLive: Bool
        /// The speed while playing (1 is normal), so the lock screen's
        /// progress moves at the right pace when the player plays faster or
        /// slower.
        public var rate: Double

        public init(
            title: String, artist: String? = nil, album: String? = nil, artwork: UIImage? = nil,
            duration: Double? = nil, position: Double? = nil, playing: Bool,
            queueIndex: Int? = nil, queueCount: Int? = nil,
            isLive: Bool = false, rate: Double = 1.0
        ) {
            self.title = title
            self.artist = artist
            self.album = album
            self.artwork = artwork
            self.duration = duration
            self.position = position
            self.playing = playing
            self.queueIndex = queueIndex
            self.queueCount = queueCount
            self.isLive = isLive
            self.rate = rate
        }
    }

    /// The buttons the player supports. Each one given shows; the rest
    /// don't.
    public struct Commands {
        public var play: (@MainActor () -> Void)?
        public var pause: (@MainActor () -> Void)?
        public var toggle: (@MainActor () -> Void)?
        public var next: (@MainActor () -> Void)?
        public var previous: (@MainActor () -> Void)?
        /// Moves to a time, in seconds (the progress bar).
        public var seek: (@MainActor (Double) -> Void)?
        /// Skip back and forward, with how far each moves, in seconds.
        public var skipBackward: (interval: Double, run: @MainActor () -> Void)?
        public var skipForward: (interval: Double, run: @MainActor () -> Void)?

        public init(
            play: (@MainActor () -> Void)? = nil,
            pause: (@MainActor () -> Void)? = nil,
            toggle: (@MainActor () -> Void)? = nil,
            next: (@MainActor () -> Void)? = nil,
            previous: (@MainActor () -> Void)? = nil,
            seek: (@MainActor (Double) -> Void)? = nil,
            skipBackward: (interval: Double, run: @MainActor () -> Void)? = nil,
            skipForward: (interval: Double, run: @MainActor () -> Void)? = nil
        ) {
            self.play = play
            self.pause = pause
            self.toggle = toggle
            self.next = next
            self.previous = previous
            self.seek = seek
            self.skipBackward = skipBackward
            self.skipForward = skipForward
        }
    }

    private let isPlaying: @MainActor () -> Bool
    private let systemPause: @MainActor () -> Void
    private let systemPlay: @MainActor () -> Void
    private var rules = ForgeAudioRules()
    private let targets = Targets()
    // Read again only in deinit, which isn't on the main actor.
    nonisolated(unsafe) private var observers: [NSObjectProtocol] = []

    /// `isPlaying`, `pause` and `play` are what the system's rules use:
    /// pause when a call, Siri or an alarm begins, or when headphones go;
    /// play again after an interruption only if it was playing and iOS
    /// says it may.
    public init(
        isPlaying: @escaping @MainActor () -> Bool,
        pause: @escaping @MainActor () -> Void,
        play: @escaping @MainActor () -> Void
    ) {
        self.isPlaying = isPlaying
        systemPause = pause
        systemPlay = play
        watchSystem()
    }

    deinit {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
    }

    /// Shows `commands` on the lock screen and headphones, replacing the
    /// ones this player gave before. Call it when playing starts.
    public func commands(_ commands: Commands) {
        targets.removeAll()
        let center = MPRemoteCommandCenter.shared()
        func add(_ command: MPRemoteCommand, _ run: (@MainActor () -> Void)?) {
            command.isEnabled = run != nil
            guard let run else { return }
            targets.add(command) { _ in
                MainActor.assumeIsolated { run() }
                return .success
            }
        }
        add(center.playCommand, commands.play)
        add(center.pauseCommand, commands.pause)
        add(center.togglePlayPauseCommand, commands.toggle)
        add(center.nextTrackCommand, commands.next)
        add(center.previousTrackCommand, commands.previous)
        center.changePlaybackPositionCommand.isEnabled = commands.seek != nil
        if let seek = commands.seek {
            targets.add(center.changePlaybackPositionCommand) { event in
                guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
                let position = event.positionTime
                MainActor.assumeIsolated { seek(position) }
                return .success
            }
        }
        if let skip = commands.skipBackward {
            center.skipBackwardCommand.preferredIntervals = [NSNumber(value: skip.interval)]
        }
        add(center.skipBackwardCommand, commands.skipBackward?.run)
        if let skip = commands.skipForward {
            center.skipForwardCommand.preferredIntervals = [NSNumber(value: skip.interval)]
        }
        add(center.skipForwardCommand, commands.skipForward?.run)
    }

    /// Whether this player has its buttons on the lock screen now.
    public var hasCommands: Bool { !targets.isEmpty }

    /// Writes what the lock screen shows. The media type, default rate and
    /// whether it is live are set every time.
    public func update(_ info: Info) {
        let rate = info.rate.isFinite && info.rate > 0 ? info.rate : 1.0
        var values: [String: Any] = [
            MPMediaItemPropertyTitle: info.title,
            MPNowPlayingInfoPropertyPlaybackRate: info.playing ? rate : 0.0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
            MPNowPlayingInfoPropertyIsLiveStream: info.isLive,
        ]
        if let artist = info.artist { values[MPMediaItemPropertyArtist] = artist }
        if let album = info.album { values[MPMediaItemPropertyAlbumTitle] = album }
        if !info.isLive {
            if let duration = info.duration, duration > 0 { values[MPMediaItemPropertyPlaybackDuration] = duration }
            if let position = info.position { values[MPNowPlayingInfoPropertyElapsedPlaybackTime] = position }
        }
        if let artwork = info.artwork { values[MPMediaItemPropertyArtwork] = Self.artwork(artwork) }
        if let index = info.queueIndex, let count = info.queueCount {
            values[MPNowPlayingInfoPropertyPlaybackQueueIndex] = index
            values[MPNowPlayingInfoPropertyPlaybackQueueCount] = count
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = values
    }

    /// Clears the lock screen and removes this player's buttons. The
    /// system's rules stay until the player is freed, so a player that
    /// plays again needs only `commands` and `update`.
    public func end() {
        targets.removeAll()
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    }

    /// Made outside the main actor: MediaPlayer asks for the picture on a
    /// background queue, and a closure made on the main actor traps there.
    nonisolated private static func artwork(_ image: UIImage) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: image.size) { _ in image }
    }

    private func watchSystem() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            let info = note.userInfo
            let type = (info?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init(rawValue:))
            let options = AVAudioSession.InterruptionOptions(
                rawValue: info?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
            )
            let event: ForgeAudioEvent
            switch type {
            case .began: event = .interruptionBegan
            case .ended: event = .interruptionEnded(shouldResume: options.contains(.shouldResume))
            default: return
            }
            MainActor.assumeIsolated { self?.handle(event) }
        })
        observers.append(center.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
        ) { [weak self] note in
            let reason = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
            guard reason == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue else { return }
            MainActor.assumeIsolated { self?.handle(.outputGone) }
        })
    }

    private func handle(_ event: ForgeAudioEvent) {
        switch rules.react(to: event, playing: isPlaying()) {
        case .pause: systemPause()
        case .play: systemPlay()
        case .nothing: break
        }
    }

    /// The handlers this player added, removed one by one rather than with
    /// `removeTarget(nil)`, which would take another app's away too.
    private final class Targets {
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
}
#endif
