#if os(iOS)
@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import AVFoundation
import UIKit
import Observation

private let log = ForgeLog("LiveRadio")

/// Plays one live station (docs/podcasts-and-radio.md). A live station is
/// not a song: it never enters the queue. Tuning in sets the music aside
/// (the queue stays, paused, and comes back when music plays again) and
/// stops a generated station's top-up. There is no scrubber, skip or
/// repeat; pausing stops the stream and playing again rejoins it live,
/// rather than playing an old buffer.
///
/// When a stream fails, its listed address is tried, then the station is
/// looked up again by its UUID (it may have moved its stream), then it says
/// the station isn't broadcasting. A stream that never starts counts as
/// failed after `connectTimeout`. One that stops sending (no error, just
/// waiting) is rejoined once after `stallTimeout`, then counts as failed;
/// so does one that ends, which a live stream shouldn't.
@Observable
@MainActor
final class LivePlayer {
    private(set) var station: LiveStation?
    private(set) var isPlaying = false
    /// Connecting to the stream, or waiting for it to fill.
    private(set) var isConnecting = false
    /// Why the station isn't playing, in words to show.
    private(set) var failure: String?

    @ObservationIgnored private let player = AVPlayer()
    @ObservationIgnored private var statusObserver: NSKeyValueObservation?
    @ObservationIgnored private var waitObserver: NSKeyValueObservation?
    /// Addresses not yet tried for this station, after the one playing.
    @ObservationIgnored private var fallbacks: [String] = []
    /// Whether the station was looked up again after a failure already.
    @ObservationIgnored private var lookedUp = false
    @ObservationIgnored private var tuneNumber = 0
    /// The address playing now, to rejoin it after a stall.
    @ObservationIgnored private var address: String?
    /// When this load started playing; nil until it has.
    @ObservationIgnored private var startedAt: Date?
    /// Whether this address was rejoined after a stall already.
    @ObservationIgnored private var rejoined = false
    /// Counts down while the stream waits; cancelled when it plays.
    @ObservationIgnored private var watchdog: Task<Void, Never>?
    @ObservationIgnored private var endObservers: [NSObjectProtocol] = []

    /// How long a stream may take to start before the next address is tried.
    static let connectTimeout: Duration = .seconds(15)
    /// How long a playing stream may wait for data before it is rejoined.
    static let stallTimeout: Duration = .seconds(20)
    /// Played this long since it was rejoined, a later stall may rejoin again.
    static let steadyAfter: TimeInterval = 60
    @ObservationIgnored private var artwork: UIImage?
    @ObservationIgnored private var artworkTask: Task<Void, Never>?
    @ObservationIgnored private lazy var nowPlaying = ForgeNowPlaying(
        isPlaying: { [weak self] in self?.isPlaying == true },
        pause: { [weak self] in self?.pause() },
        play: { [weak self] in self?.play() }
    )
    @ObservationIgnored private weak var radioModel: LiveRadioModel?
    @ObservationIgnored private weak var music: TakePlayer?
    @ObservationIgnored private weak var radio: Radio?

    var isLive: Bool { station != nil }

    init() {
        player.automaticallyWaitsToMinimizeStalling = true
    }

    /// What tuning in works with: the stations' data, the music player it
    /// sets aside and the generated station it stops. Set once by the app.
    func attach(model: LiveRadioModel, music: TakePlayer, radio: Radio) {
        radioModel = model
        self.music = music
        self.radio = radio
    }

    /// Tunes in to `station`: the music queue is set aside, a generated
    /// station stops, and the stream starts.
    func tune(_ station: LiveStation) async {
        guard let model = radioModel else { return }
        tuneNumber += 1
        let number = tuneNumber
        radio?.stop()
        music?.stepAside()
        self.station = station
        failure = nil
        isConnecting = true
        lookedUp = false
        setUpCommands()
        loadArtwork(station.artworkUrl)
        let tuned: LiveStation
        do {
            tuned = try await model.tune(station)
        } catch SoundscapeError.NotInLibrary {
            guard number == tuneNumber else { return }
            fail("Radio Browser no longer lists this station.")
            return
        } catch {
            guard number == tuneNumber else { return }
            fail(ListenModel.message(error))
            return
        }
        guard number == tuneNumber else { return }
        self.station = tuned
        start(tuned, addresses: [tuned.streamUrl, tuned.listedUrl])
    }

    func toggle() {
        isPlaying || isConnecting ? pause() : play()
    }

    /// Plays again from now (live), not from where it paused.
    func play() {
        guard let station else { return }
        lookedUp = false
        start(station, addresses: [station.streamUrl, station.listedUrl])
    }

    /// Stops the stream; the station stays, to play again.
    func pause() {
        tuneNumber += 1
        unload()
        isPlaying = false
        isConnecting = false
        updateNowPlaying()
    }

    /// Ends live radio: the lock screen goes back to the music, paused.
    func stop() {
        guard station != nil else { return }
        tuneNumber += 1
        unload()
        station = nil
        isPlaying = false
        isConnecting = false
        failure = nil
        artworkTask?.cancel()
        artwork = nil
        nowPlaying.end()
    }

    // MARK: Playing

    private func start(_ station: LiveStation, addresses: [String]) {
        var seen = Set<String>()
        let unique = addresses.filter { !$0.isEmpty && seen.insert($0).inserted }
        guard let first = unique.first else {
            fail("This station gives no address to play.")
            return
        }
        fallbacks = Array(unique.dropFirst())
        failure = nil
        load(first)
    }

    /// Plays `address`; `rejoining` plays the same one again after a stall.
    private func load(_ address: String, rejoining: Bool = false) {
        guard let url = URL(string: address) else {
            next()
            return
        }
        do {
            try ForgeAudioSession.activate(.music)
        } catch {
            log.warn("audio session: \(error)")
        }
        unload()
        self.address = address
        // A rejoin that doesn't start counts as failed, like a new address.
        startedAt = nil
        if !rejoining { rejoined = false }
        let item = AVPlayerItem(url: url)
        // A few seconds buffered, then play: live, not a song to cache.
        item.preferredForwardBufferDuration = 8
        statusObserver = item.observe(\.status, options: [.new]) { [weak self] item, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    // Only the stream playing now: an old item's late news is dropped.
                    guard let self, item.status == .failed, self.player.currentItem === item else { return }
                    log.warn("stream failed")
                    self.next()
                }
            }
        }
        waitObserver = player.observe(\.timeControlStatus, options: [.new]) { [weak self] player, _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, player.currentItem === item else { return }
                    let status = player.timeControlStatus
                    self.isConnecting = status == .waitingToPlayAtSpecifiedRate
                    switch status {
                    case .playing:
                        self.failure = nil
                        self.startedAt = Date()
                        self.watchdog?.cancel()
                        self.watchdog = nil
                    case .waitingToPlayAtSpecifiedRate where self.watchdog == nil:
                        self.watch(item, for: self.startedAt == nil ? Self.connectTimeout : Self.stallTimeout)
                    default:
                        break
                    }
                }
            }
        }
        // A live stream has no end: one that ends, or can't carry on, dropped.
        let itemID = ObjectIdentifier(item)
        for name in [AVPlayerItem.didPlayToEndTimeNotification, AVPlayerItem.failedToPlayToEndTimeNotification] {
            endObservers.append(NotificationCenter.default.addObserver(
                forName: name, object: item, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, self.player.currentItem.map(ObjectIdentifier.init) == itemID else { return }
                    log.warn("stream ended")
                    self.stalled()
                }
            })
        }
        player.replaceCurrentItem(with: item)
        player.play()
        isPlaying = true
        isConnecting = true
        watch(item, for: Self.connectTimeout)
        updateNowPlaying()
    }

    /// Calls `stalled()` if `item` is still waiting after `limit`.
    private func watch(_ item: AVPlayerItem, for limit: Duration) {
        watchdog?.cancel()
        watchdog = Task { [weak self] in
            try? await Task.sleep(for: limit)
            guard !Task.isCancelled, let self, self.player.currentItem === item,
                  self.player.timeControlStatus != .playing else { return }
            self.watchdog = nil
            self.stalled()
        }
    }

    /// The stream stopped sending, or never started. One that played is
    /// rejoined once (again once it has played steadily since); otherwise
    /// the next address is tried.
    private func stalled() {
        if let startedAt, rejoined, Date().timeIntervalSince(startedAt) > Self.steadyAfter {
            rejoined = false
        }
        guard let address, startedAt != nil, !rejoined else {
            log.warn(startedAt == nil ? "stream didn't start in time" : "stream stalled again")
            next()
            return
        }
        log.warn("stream stalled; rejoining")
        rejoined = true
        load(address, rejoining: true)
    }

    /// The next address for the station: its listed one, then (once) its
    /// details looked up again by UUID, then a failure.
    private func next() {
        if !fallbacks.isEmpty {
            load(fallbacks.removeFirst())
            return
        }
        guard let station, let radioModel, !lookedUp else {
            fail("This station isn't broadcasting right now.")
            return
        }
        lookedUp = true
        let number = tuneNumber
        Task {
            let found: LiveStation?
            do {
                found = try await radioModel.failed(station.uuid)
            } catch {
                // Offline, or Radio Browser unreachable: say that, not that
                // the station is gone.
                guard number == tuneNumber, self.station?.uuid == station.uuid else { return }
                fail(ListenModel.message(error))
                return
            }
            guard number == tuneNumber, self.station?.uuid == station.uuid else { return }
            guard let found, found.streamUrl != station.streamUrl || found.listedUrl != station.listedUrl else {
                fail(found == nil
                     ? "Radio Browser no longer lists this station."
                     : "This station isn't broadcasting right now.")
                return
            }
            self.station = found
            start(found, addresses: [found.streamUrl, found.listedUrl])
        }
    }

    private func fail(_ message: String) {
        unload()
        isPlaying = false
        isConnecting = false
        failure = message
        updateNowPlaying()
    }

    private func unload() {
        statusObserver = nil
        waitObserver = nil
        watchdog?.cancel()
        watchdog = nil
        for observer in endObservers { NotificationCenter.default.removeObserver(observer) }
        endObservers = []
        player.pause()
        player.replaceCurrentItem(with: nil)
    }

    // MARK: Lock screen

    private func setUpCommands() {
        guard !nowPlaying.hasCommands else { return }
        nowPlaying.commands(.init(
            play: { [weak self] in self?.play() },
            pause: { [weak self] in self?.pause() },
            toggle: { [weak self] in self?.toggle() }
        ))
    }

    private func updateNowPlaying() {
        guard let station else { return }
        nowPlaying.update(.init(
            title: station.name,
            artist: failure ?? (station.country.isEmpty ? "Live Radio" : station.country),
            album: "Live Radio",
            artwork: artwork,
            playing: isPlaying,
            isLive: true
        ))
    }

    private func loadArtwork(_ url: String?) {
        artworkTask?.cancel()
        artwork = nil
        guard let url, let address = URL(string: url) else { return }
        artworkTask = Task { [weak self] in
            let picture = await ForgeImages.thumbnail(from: address, maxPixels: 512)
            guard let picture, !Task.isCancelled, let self else { return }
            self.artwork = UIImage(cgImage: picture)
            self.updateNowPlaying()
        }
    }
}
#endif
