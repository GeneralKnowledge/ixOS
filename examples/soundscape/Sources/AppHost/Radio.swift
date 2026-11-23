#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import Observation
import SwiftUI

/// Nonstop music from a station (docs/design.md, Radio): Rust mixes a
/// batch, the player queues it, and a new batch is asked for while a few
/// songs are still to come, so playback never runs dry and AVPlayer always
/// has the next song loading. A failed top-up is retried with a growing
/// wait while the station stays on; when the retries run out, or nothing
/// fits the station, it says so with Try Again and Stop. Playing anything
/// else ends the radio.
@Observable
@MainActor
final class Radio {
    enum Status: Equatable {
        /// On air (or off when there is no seed).
        case playing
        /// A top-up failed; trying again (attempt `n` of `retries.count`).
        case retrying(attempt: Int, message: String)
        /// The retries ran out; the queue may end. Try Again or Stop.
        case failed(String)
        /// Nothing fits this station: trying again won't help.
        case exhausted
    }

    /// The station on air, with the name to show.
    private(set) var seed: StationSeed?
    private(set) var title: String?
    private(set) var status: Status = .playing
    private(set) var isTuning = false
    /// The latest `start`: an older one finishing must not clear `isTuning`.
    private var startNumber = 0

    /// Songs given so far, so a batch never repeats them.
    private var played: [String] = []
    private var artists: [String] = []
    /// The top-up in progress, retries included. Radio owns it, so a
    /// view's task ending (the queue moved, the screen changed) never cuts
    /// a backoff short or restarts its count.
    private var topUpTask: Task<Void, Never>?
    /// Tells a finished top-up whether it is still the current one.
    private var topUpNumber = 0

    /// Ask for more while fewer than this many songs are still to come.
    static let ahead = 4
    /// The waits between tries of a failed top-up, in seconds.
    static let retries: [Double] = [2, 5, 10, 20, 40]

    var isOn: Bool { seed != nil }

    var isRetrying: Bool {
        if case .retrying = status { return true }
        return false
    }

    /// What to tell the person about the station, if anything.
    var problem: String? {
        switch status {
        case .playing: return nil
        case .retrying(let attempt, _): return "Can't get more songs. Trying again (\(attempt) of \(Self.retries.count))…"
        case .failed(let message): return "Can't get more songs: \(message)"
        case .exhausted: return "Nothing more fits this station."
        }
    }

    /// Starts `seed` from its first batch.
    func start(_ seed: StationSeed, title: String, model: ListenModel, player: TakePlayer) async {
        cancelTopUp()
        self.seed = seed
        self.title = title
        status = .playing
        played = []
        artists = []
        startNumber += 1
        let number = startNumber
        isTuning = true
        defer { if number == startNumber { isTuning = false } }
        switch await model.stationBatch(seed, played: [], recentArtists: []) {
        case .success(let tracks):
            guard self.seed == seed else { return }
            note(tracks)
            player.play(tracks)
        case .failure(let f):
            guard !f.stopped, self.seed == seed else { return }
            stop()
            let message = f.nothingToPlay
                ? "Nothing in your library fits it yet, and there's nothing new to add. Find music in Discover, or add a Jamendo client ID."
                : f.message
            model.alert = .failure(ForgeFailure(title: "Can't Start \(title)", message: message))
        }
    }

    /// Called when the queue moves on: tops it up, or ends the radio when
    /// something else is playing now. A failed top-up is tried again after
    /// each wait in `retries` while the station is still on air; a call
    /// while one is under way leaves it alone.
    func follow(model: ListenModel, player: TakePlayer) async {
        guard let seed, !isTuning else { return }
        // Stopped (nothing playing) or something else playing ends the
        // radio: a top-up would otherwise start the music again.
        guard let current = player.current?.trackId, played.contains(current) else {
            stop()
            return
        }
        guard topUpTask == nil, player.upcoming.count < Self.ahead else { return }
        if case .exhausted = status { return }
        if case .failed = status { return }
        beginTopUp(seed, model: model, player: player)
    }

    /// Try Again after the retries ran out or the station was empty.
    func retry(model: ListenModel, player: TakePlayer) async {
        guard let seed, topUpTask == nil else { return }
        status = .playing
        beginTopUp(seed, model: model, player: player)
    }

    private func beginTopUp(_ seed: StationSeed, model: ListenModel, player: TakePlayer) {
        topUpNumber += 1
        let number = topUpNumber
        topUpTask = Task { [weak self] in
            await self?.topUp(seed, model: model, player: player)
            guard let self, self.topUpNumber == number else { return }
            self.topUpTask = nil
        }
    }

    private func cancelTopUp() {
        topUpTask?.cancel()
        topUpTask = nil
        topUpNumber += 1
    }

    private func topUp(_ seed: StationSeed, model: ListenModel, player: TakePlayer) async {
        var attempt = 0
        while self.seed == seed, !Task.isCancelled {
            switch await model.stationBatch(seed, played: played, recentArtists: artists) {
            case .success(let tracks):
                guard self.seed == seed else { return }
                status = .playing
                note(tracks)
                player.add(tracks, next: false)
                return
            case .failure(let f):
                // Stopped (the radio went off, or the app left the
                // foreground): the next follow starts again.
                if f.stopped { return }
                if f.nothingToPlay {
                    status = .exhausted
                    return
                }
                guard attempt < Self.retries.count else {
                    status = .failed(f.message)
                    return
                }
                attempt += 1
                status = .retrying(attempt: attempt, message: f.message)
                try? await Task.sleep(for: .seconds(Self.retries[attempt - 1]))
            }
        }
    }

    func stop() {
        cancelTopUp()
        seed = nil
        title = nil
        status = .playing
    }

    private func note(_ tracks: [QueueTrack]) {
        played += tracks.map(\.trackId)
        artists = Array((artists + tracks.map(\.artist)).suffix(8))
    }
}

/// The radio's trouble, with Try Again and Stop: under Now Playing's radio
/// row and at the top of Listen.
struct RadioProblem: View {
    let radio: Radio
    let model: ListenModel
    let player: TakePlayer

    var body: some View {
        if let problem = radio.problem {
            VStack(alignment: .leading, spacing: 8) {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .font(.footnote)
                    .accessibilityIdentifier("radio.problem")
                if radio.isRetrying {
                    ProgressView().controlSize(.small)
                } else {
                    HStack(spacing: 12) {
                        Button("Try Again") {
                            Task { await radio.retry(model: model, player: player) }
                        }
                        .accessibilityIdentifier("radio.retry")
                        Button("Stop Radio", role: .destructive) { radio.stop() }
                            .accessibilityIdentifier("radio.stop")
                    }
                    .font(.footnote.weight(.semibold))
                    .buttonStyle(.bordered)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// A station on a shelf: its colours from its name, its covers when it has
/// some, and its name large.
struct StationTile: View {
    let station: Station
    var width: CGFloat = 240

    var body: some View {
        let hue = ArtworkPalette.standInHue(station.title)
        VStack(alignment: .leading, spacing: 6) {
            ZStack(alignment: .bottomLeading) {
                LinearGradient(
                    colors: [Color(hue: hue, saturation: 0.65, brightness: 0.7),
                             Color(hue: (hue + 0.1).truncatingRemainder(dividingBy: 1), saturation: 0.8, brightness: 0.35)],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )
                HStack(spacing: -18) {
                    ForEach(Array(station.artworkUrls.prefix(3).enumerated()), id: \.offset) { _, url in
                        Artwork(title: station.title, url: url, size: 64)
                            .shadow(color: .black.opacity(0.3), radius: 6, y: 3)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                .padding(12)
                VStack(alignment: .leading, spacing: 2) {
                    Label(station.saved ? "Saved Station" : "Radio",
                          systemImage: station.saved ? "pin.fill" : "dot.radiowaves.left.and.right")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.8))
                    Text(station.title.replacingOccurrences(of: " Station", with: ""))
                        .font(.system(.title2, design: .rounded).weight(.heavy))
                        .foregroundStyle(.white)
                        .lineLimit(2)
                        .minimumScaleFactor(0.7)
                }
                .padding(14)
            }
            .frame(width: width, height: width * 0.6)
            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            // A picture with words on it: its size is fixed, so its text
            // stops growing where it would no longer fit.
            .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            Text(station.subtitle)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .frame(width: width, alignment: .leading)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(station.title), \(station.subtitle)")
    }
}

extension Station: Identifiable {
    public var id: StationSeed { seed }
}
#endif
