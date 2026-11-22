#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import Observation
import SwiftUI

/// Nonstop music from a station (docs/design.md, Radio): Rust mixes a
/// batch, the player queues it, and a new batch is asked for while a few
/// songs are still to come, so playback never runs dry and AVPlayer always
/// has the next song loading. Playing anything else ends the radio.
@Observable
@MainActor
final class Radio {
    /// The station on air, with the name to show.
    private(set) var seed: StationSeed?
    private(set) var title: String?
    private(set) var failure: String?
    private(set) var isTuning = false

    /// Songs given so far, so a batch never repeats them.
    private var played: [String] = []
    private var artists: [String] = []
    private var fetching = false

    /// Ask for more while fewer than this many songs are still to come.
    static let ahead = 4

    var isOn: Bool { seed != nil }

    /// Starts `seed` from its first batch.
    func start(_ seed: StationSeed, title: String, model: ListenModel, player: TakePlayer) async {
        self.seed = seed
        self.title = title
        failure = nil
        played = []
        artists = []
        isTuning = true
        defer { isTuning = false }
        switch await model.stationBatch(seed, played: [], recentArtists: []) {
        case .success(let tracks):
            guard self.seed == seed else { return }
            note(tracks)
            player.play(tracks)
        case .failure(let f):
            guard !f.stopped else { return }
            failure = f.message
            stop()
            model.alert = .failure(ForgeFailure(title: "Can't Start \(title)", message: f.message))
        }
    }

    /// Called when the queue moves on: tops it up, or ends the radio when
    /// something else is playing now.
    func follow(model: ListenModel, player: TakePlayer) async {
        guard let seed, !fetching, !isTuning else { return }
        if let current = player.current?.trackId, !played.contains(current) {
            stop()
            return
        }
        guard player.upcoming.count < Self.ahead else { return }
        fetching = true
        defer { fetching = false }
        if case .success(let tracks) = await model.stationBatch(seed, played: played, recentArtists: artists),
           self.seed == seed {
            note(tracks)
            player.add(tracks, next: false)
        }
    }

    func stop() {
        seed = nil
        title = nil
    }

    private func note(_ tracks: [QueueTrack]) {
        played += tracks.map(\.trackId)
        artists = Array((artists + tracks.map(\.artist)).suffix(8))
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
                    Label("Radio", systemImage: "dot.radiowaves.left.and.right")
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
