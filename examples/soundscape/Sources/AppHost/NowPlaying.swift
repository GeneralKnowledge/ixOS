#if os(iOS)
@preconcurrency import SoundscapeFFI
import AVKit
import ForgeAppKit
import ForgeUI
import MediaPlayer
import SwiftUI
import UIKit

/// The full player (docs/design.md, S3): the song's artwork as large as the
/// screen allows over its own colours, the position, the transport, volume,
/// and lyrics, AirPlay and the queue along the bottom. Lyrics show in
/// place of the artwork. In an A/B comparison, the A and B switch takes
/// the transport's place.
struct NowPlayingView: View {
    let model: ListenModel
    let player: TakePlayer
    /// Closes the player and opens a screen in the tab underneath.
    let go: (ListenRoute) -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var palette = ArtworkPalette.standIn(title: "")
    @State private var showingLyrics = false
    @State private var showingQueue = false
    @State private var addingToPlaylist = false
    @Namespace private var artworkSpace

    var body: some View {
        ZStack {
            NowPlayingBackground(palette: palette, current: player.current)
            if let current = player.current {
                VStack(spacing: 0) {
                    grabber
                    if showingLyrics, let lyrics = current.lyrics {
                        compactHeader(current)
                            .padding(.top, 8)
                        LyricsScroll(lyrics: lyrics, player: player, sung: .white, edge: 0)
                            .mask(fadeEdges)
                    } else {
                        bigArtwork(current)
                        titleRow(current)
                    }
                    rating(current)
                    NowPlayingScrubber(player: player)
                        .padding(.top, 14)
                    if let note = sleepNote {
                        Label(note, systemImage: "moon.zzz")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.top, 2)
                            .accessibilityIdentifier("nowPlayingSheet.sleepNote")
                    }
                    if player.isComparing { abSwitch } else { transport }
                    VolumeSlider()
                        .padding(.top, 4)
                    bottomBar(current)
                }
                .padding(.horizontal, SoundscapeMetrics.nowPlayingEdge)
                .padding(.bottom, 8)
            }
        }
        .environment(\.colorScheme, .dark)
        .task(id: player.current?.artworkUrl ?? player.current?.song) {
            guard let current = player.current else { return }
            let found = await ArtworkPaletteLoader.palette(title: current.song, url: current.artworkUrl)
            withAnimation(SoundscapeMotion.palette(reduced: reduceMotion)) { palette = found }
        }
        .onChange(of: player.current?.lyrics == nil) { _, none in
            if none { showingLyrics = false }
        }
        .sheet(isPresented: $showingQueue) {
            QueueSheet(player: player)
        }
        .sheet(isPresented: $addingToPlaylist) {
            if let trackId = player.current?.trackId {
                AddToPlaylistSheet(model: model, trackIds: [trackId])
            }
        }
        .forgeFeedback(model.feedback)
    }

    // MARK: Parts

    /// A handle to pull the player down by, and a button that does the same.
    private var grabber: some View {
        ZStack {
            Capsule()
                .fill(.white.opacity(0.35))
                .frame(width: 40, height: 5)
                .accessibilityHidden(true)
            HStack {
                Button { dismiss() } label: {
                    Image(systemName: "chevron.down")
                        .font(.body.weight(.semibold))
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("Close")
                .accessibilityIdentifier("nowPlayingSheet.close")
                Spacer()
            }
            .padding(.horizontal, -12)
        }
        .foregroundStyle(.white.opacity(0.8))
        .padding(.top, 4)
        .contentShape(Rectangle())
        // Pulling the top down closes the player, as the zoom does on iOS 18.
        .gesture(DragGesture(minimumDistance: 20).onEnded { drag in
            if drag.translation.height > 80 { dismiss() }
        })
    }

    private func bigArtwork(_ current: TakePlayer.Loaded) -> some View {
        GeometryReader { proxy in
            let side = max(min(proxy.size.width, proxy.size.height), 1)
            let playing = player.isPlaying || player.isComparing
            Artwork(title: current.song, url: current.artworkUrl, size: side)
                .id(current.id)
                .transition(.opacity)
                .matchedGeometryEffect(id: "artwork", in: artworkSpace)
                .shadow(color: .black.opacity(0.35), radius: 24, y: 12)
                .scaleEffect(playing || reduceMotion ? 1 : 0.86)
                .animation(SoundscapeMotion.spring(reduced: reduceMotion), value: playing)
                .animation(reduceMotion ? nil : .easeInOut(duration: 0.35), value: current.id)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(.vertical, 20)
        .accessibilityHidden(true)
    }

    /// The small artwork row lyrics are read under.
    private func compactHeader(_ current: TakePlayer.Loaded) -> some View {
        HStack(spacing: 12) {
            Artwork(title: current.song, url: current.artworkUrl, size: 56)
                .matchedGeometryEffect(id: "artwork", in: artworkSpace)
            titles(current, font: .headline)
            Spacer(minLength: 0)
            titleActions(current)
        }
        .padding(.bottom, 8)
    }

    private func titleRow(_ current: TakePlayer.Loaded) -> some View {
        HStack(alignment: .center, spacing: 12) {
            titles(current, font: .title3.bold())
            Spacer(minLength: 0)
            titleActions(current)
        }
    }

    private func titles(_ current: TakePlayer.Loaded, font: Font) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(player.isComparing ? "Comparing · \(player.active == .a ? "A" : "B")" : current.song)
                .font(font)
                .foregroundStyle(.white)
                .lineLimit(1)
            Text(subtitle(current))
                .font(font == .headline ? .subheadline : .title3)
                .foregroundStyle(.white.opacity(0.6))
                .lineLimit(1)
        }
        .accessibilityElement(children: .combine)
    }

    private func subtitle(_ current: TakePlayer.Loaded) -> String {
        if player.isComparing { return current.song }
        return [current.artist, current.album].compactMap { $0 }.joined(separator: " · ")
            .nonEmpty ?? current.label
    }

    @ViewBuilder private func titleActions(_ current: TakePlayer.Loaded) -> some View {
        if let trackId = current.trackId, !player.isComparing {
            FavouriteButton(model: model, id: trackId, identifier: "nowPlayingSheet.favourite")
                .labelStyle(.iconOnly)
                .tint(.white)
                .font(.title3)
                .frame(width: 36, height: 36)
                .background(.white.opacity(0.14), in: Circle())
        }
        moreMenu(current)
    }

    /// Everything that isn't playback: the playlist, where the song comes
    /// from, more like it, the sleep timer, how songs join, and Stop.
    private func moreMenu(_ current: TakePlayer.Loaded) -> some View {
        Menu {
            if let trackId = current.trackId, !player.isComparing {
                Button("Add to Playlist", systemImage: "text.badge.plus") { addingToPlaylist = true }
                    .accessibilityIdentifier("nowPlayingSheet.addToPlaylist")
                if let track = model.track(trackId)?.track {
                    Section {
                        if let album = model.library.albums.first(where: { $0.id == track.albumId }) {
                            Button("Go to Album", systemImage: "square.stack") { go(.album(album)) }
                                .accessibilityIdentifier("nowPlayingSheet.goToAlbum")
                        }
                        if let artist = model.library.artists.first(where: { $0.id == track.artistId }) {
                            Button("Go to Artist", systemImage: "music.mic") { go(.artist(artist)) }
                                .accessibilityIdentifier("nowPlayingSheet.goToArtist")
                        }
                    }
                }
                if trackId.hasPrefix("jamendo:"), model.jamendoClientId != nil {
                    Section("On Jamendo") {
                        Button(current.artist.map { "More by \($0)" } ?? "More by This Artist",
                               systemImage: "music.mic") {
                            go(.jamendoMore(trackId: trackId, by: .artist))
                        }
                        .accessibilityIdentifier("nowPlayingSheet.moreByArtist")
                        Button("More in This Genre", systemImage: "guitars") {
                            go(.jamendoMore(trackId: trackId, by: .genre))
                        }
                        .accessibilityIdentifier("nowPlayingSheet.moreInGenre")
                    }
                }
            }
            Section {
                SleepTimerMenu(player: player)
                if !player.isComparing {
                    CrossfadeMenu(model: model, player: player)
                }
            }
            Button("Stop", systemImage: "stop.fill", role: .destructive) {
                player.stop()
                dismiss()
            }
            .accessibilityIdentifier("nowPlayingSheet.stop")
        } label: {
            Image(systemName: "ellipsis")
                .font(.body.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(.white.opacity(0.14), in: Circle())
                .contentShape(Circle())
        }
        .accessibilityLabel("More")
        .accessibilityIdentifier("nowPlayingSheet.more")
    }

    /// Stars for a generated take (a take server recording), to rate it
    /// while it plays.
    @ViewBuilder private func rating(_ current: TakePlayer.Loaded) -> some View {
        if !player.isComparing, let trackId = current.trackId,
           let recording = model.track(trackId)?.recordings.first(where: { $0.id == current.id }),
           recording.kind == .generated {
            HStack {
                ForgeRating("Rate this take", value: Int(recording.stars), identifier: "nowPlayingSheet.rating") { n in
                    Task { _ = await model.rate(recording, of: trackId, stars: n ?? 0) }
                }
                Spacer()
            }
            .padding(.top, 6)
        }
    }

    private var transport: some View {
        HStack {
            Spacer()
            TransportButton(systemImage: "backward.fill", size: 30, label: "Previous",
                            identifier: "nowPlayingSheet.previous") { player.previous() }
            Spacer()
            TransportButton(systemImage: player.isPlaying ? "pause.fill" : "play.fill", size: 46,
                            label: player.isPlaying ? "Pause" : "Play",
                            identifier: "nowPlayingSheet.playPause") { player.toggle() }
            Spacer()
            TransportButton(systemImage: "forward.fill", size: 30, label: "Next",
                            identifier: "nowPlayingSheet.next") { player.next() }
                .disabled(!player.hasNext)
            Spacer()
        }
        .padding(.vertical, 18)
    }

    private var abSwitch: some View {
        HStack(spacing: 24) {
            ForEach([TakePlayer.Side.a, .b], id: \.self) { side in
                Button { player.hear(side) } label: {
                    Text(side == .a ? "A" : "B")
                        .font(.largeTitle.bold())
                        .frame(width: 72, height: 72)
                }
                .buttonStyle(.forgeChoice(isSelected: player.active == side, shape: .circle))
                .accessibilityLabel(side == .a ? "Hear A" : "Hear B")
                .accessibilityIdentifier(side == .a ? "nowPlayingSheet.a" : "nowPlayingSheet.b")
            }
            TransportButton(systemImage: player.isPlaying ? "pause.fill" : "play.fill", size: 46,
                            label: player.isPlaying ? "Pause" : "Play",
                            identifier: "nowPlayingSheet.playPause") { player.toggle() }
        }
        .padding(.vertical, 14)
    }

    private func bottomBar(_ current: TakePlayer.Loaded) -> some View {
        let hasLyrics = current.lyrics.map { !lyricLines(lyrics: $0).isEmpty } ?? false
        return HStack {
            Button {
                withAnimation(SoundscapeMotion.spring(reduced: reduceMotion)) { showingLyrics.toggle() }
            } label: {
                Image(systemName: showingLyrics ? "quote.bubble.fill" : "quote.bubble")
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .disabled(!hasLyrics)
            .opacity(hasLyrics ? 1 : 0.35)
            .accessibilityLabel("Lyrics")
            .accessibilityValue(showingLyrics ? "Showing" : "Hidden")
            .accessibilityIdentifier("nowPlayingSheet.lyrics")
            Spacer()
            AirPlayButton()
                .frame(width: 44, height: 44)
                .accessibilityIdentifier("nowPlayingSheet.airPlay")
            Spacer()
            Button { showingQueue = true } label: {
                Image(systemName: "list.bullet")
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .accessibilityLabel("Up Next")
            .accessibilityIdentifier("nowPlayingSheet.queue")
        }
        .font(.title3)
        .foregroundStyle(.white.opacity(0.8))
    }

    /// Lyrics fade out at the top and bottom rather than being cut.
    private var fadeEdges: some View {
        LinearGradient(
            stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.06),
                    .init(color: .black, location: 0.9), .init(color: .clear, location: 1)],
            startPoint: .top, endPoint: .bottom
        )
    }

    private var sleepNote: String? {
        if player.sleepAtEndOfSong { return "Pausing at the end of this song" }
        guard let at = player.sleepAt else { return nil }
        return "Pausing at \(at.formatted(date: .omitted, time: .shortened))"
    }
}

/// The player's backdrop: the artwork's colours top to bottom, the artwork
/// itself blurred very large over them, and a darker foot so the controls
/// always read.
private struct NowPlayingBackground: View {
    let palette: ArtworkPalette
    let current: TakePlayer.Loaded?

    var body: some View {
        ZStack {
            LinearGradient(colors: [palette.top.color, palette.bottom.color], startPoint: .top, endPoint: .bottom)
            if let current, current.artworkUrl != nil {
                GeometryReader { proxy in
                    Artwork(title: current.song, url: current.artworkUrl, size: max(proxy.size.height, 1))
                        .frame(width: proxy.size.width, height: proxy.size.height)
                        .blur(radius: 80)
                        .opacity(0.45)
                }
                .id(current.id)
                .transition(.opacity)
            }
            LinearGradient(colors: [.clear, .black.opacity(0.45)], startPoint: .center, endPoint: .bottom)
        }
        .ignoresSafeArea()
        .accessibilityHidden(true)
    }
}

/// The position as a thin bar that thickens under a finger, the time played
/// on the left and the time left on the right. VoiceOver adjusts it ten
/// seconds at a time.
private struct NowPlayingScrubber: View {
    let player: TakePlayer
    @State private var dragging: Double?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var duration: Double { max(player.duration, 1) }
    private var shown: Double { min(dragging ?? player.time, duration) }

    var body: some View {
        VStack(spacing: 6) {
            GeometryReader { proxy in
                let width = max(proxy.size.width, 1)
                ZStack(alignment: .leading) {
                    Capsule().fill(.white.opacity(0.22))
                    Capsule()
                        .fill(.white.opacity(dragging == nil ? 0.75 : 1))
                        .frame(width: width * shown / duration)
                }
                .frame(height: dragging == nil ? 6 : 12)
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            let fraction = min(max(drag.location.x / width, 0), 1)
                            if dragging == nil {
                                withAnimation(reduceMotion ? nil : .easeOut(duration: 0.15)) {
                                    dragging = fraction * duration
                                }
                            } else {
                                dragging = fraction * duration
                            }
                        }
                        .onEnded { _ in
                            if let to = dragging { player.seek(to: to) }
                            withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { dragging = nil }
                        }
                )
            }
            .frame(height: 20)
            HStack {
                Text(TakeFormat.time(shown))
                Spacer()
                Text("-" + TakeFormat.time(max(player.duration - shown, 0)))
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.white.opacity(dragging == nil ? 0.6 : 0.9))
            .accessibilityHidden(true)
        }
        .accessibilityElement()
        .accessibilityLabel("Position")
        .accessibilityValue("\(TakeFormat.time(shown)) of \(TakeFormat.time(player.duration))")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: player.skip(10)
            case .decrement: player.skip(-10)
            @unknown default: break
            }
        }
        .accessibilityIdentifier("nowPlayingSheet.position")
    }
}

/// A large transport glyph with no circle, that dims and shrinks a little
/// while pressed, and bounces when tapped.
private struct TransportButton: View {
    let systemImage: String
    let size: CGFloat
    let label: String
    let identifier: String
    let action: () -> Void

    @ScaledMetric private var scale: CGFloat = 1
    @State private var taps = 0
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Button {
            taps += 1
            action()
        } label: {
            Image(systemName: systemImage)
                .font(.system(size: size * min(scale, 1.4)))
                .contentTransition(.symbolEffect(.replace))
                .symbolEffect(.bounce, value: reduceMotion ? 0 : taps)
                .frame(width: size * 1.8, height: size * 1.6)
                .contentShape(Rectangle())
        }
        .buttonStyle(TransportPress())
        .foregroundStyle(.white)
        .accessibilityLabel(label)
        .accessibilityIdentifier(identifier)
    }
}

private struct TransportPress: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.6 : 1)
            .scaleEffect(configuration.isPressed ? 0.9 : 1)
            .background {
                Circle()
                    .fill(.white.opacity(configuration.isPressed ? 0.12 : 0))
            }
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

/// The system volume slider, which also follows the phone's volume buttons.
private struct VolumeSlider: View {
    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "speaker.fill")
            SystemVolume()
                .frame(height: 34)
            Image(systemName: "speaker.wave.3.fill")
        }
        .font(.caption)
        .foregroundStyle(.white.opacity(0.6))
        .accessibilityElement(children: .contain)
    }
}

private struct SystemVolume: UIViewRepresentable {
    func makeUIView(context: Context) -> MPVolumeView {
        let view = MPVolumeView(frame: .zero)
        view.tintColor = .white
        view.accessibilityIdentifier = "nowPlayingSheet.volume"
        return view
    }

    func updateUIView(_ view: MPVolumeView, context: Context) {}
}

/// AirPlay and Bluetooth: where the sound goes.
private struct AirPlayButton: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let view = AVRoutePickerView()
        view.tintColor = UIColor.white.withAlphaComponent(0.8)
        view.activeTintColor = .white
        view.prioritizesVideoDevices = false
        return view
    }

    func updateUIView(_ view: AVRoutePickerView, context: Context) {}
}

/// Up Next: shuffle and repeat, then the songs to come; tap one to play it
/// now, swipe or hold to take it out.
private struct QueueSheet: View {
    let player: TakePlayer
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack(spacing: 12) {
                        Button { player.toggleShuffle() } label: {
                            Label("Shuffle", systemImage: "shuffle")
                                .frame(maxWidth: .infinity, minHeight: 36)
                        }
                        .buttonStyle(.forgeChoice(isSelected: player.shuffle, shape: .tile))
                        .accessibilityIdentifier("nowPlayingSheet.shuffle")
                        Button { player.cycleRepeat() } label: {
                            Label(Self.repeatTitle(player.repeatMode),
                                  systemImage: player.repeatMode == .one ? "repeat.1" : "repeat")
                                .frame(maxWidth: .infinity, minHeight: 36)
                        }
                        .buttonStyle(.forgeChoice(isSelected: player.repeatMode != .off, shape: .tile))
                        .accessibilityLabel("Repeat")
                        .accessibilityValue(Self.repeatName(player.repeatMode))
                        .accessibilityIdentifier("nowPlayingSheet.repeat")
                    }
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                }
                if let current = player.current {
                    Section("Playing") {
                        QueueRow(title: current.song, subtitle: current.artist ?? current.label,
                                 artworkUrl: current.artworkUrl, playing: player.isPlaying)
                    }
                }
                Section(player.upcoming.isEmpty ? "Nothing Up Next" : "Up Next") {
                    ForEach(player.upcoming) { entry in
                        Button {
                            player.jump(to: entry.position)
                        } label: {
                            QueueRow(title: entry.track.title, subtitle: entry.track.artist,
                                     artworkUrl: entry.track.artworkUrl, playing: false)
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("upNext.song")
                        .forgeRowActions {
                            Button("Remove", systemImage: "minus.circle", role: .destructive) {
                                player.remove(at: entry.position)
                            }
                            .accessibilityIdentifier("upNext.remove")
                        }
                    }
                }
            }
            .navigationTitle("Up Next")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("upNext.done")
                }
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    static func repeatTitle(_ mode: RepeatMode) -> String {
        switch mode {
        case .off, .all: return "Repeat"
        case .one: return "Repeat One"
        }
    }

    static func repeatName(_ mode: RepeatMode) -> String {
        switch mode {
        case .off: return "Off"
        case .all: return "All"
        case .one: return "One"
        }
    }
}

private struct QueueRow: View {
    let title: String
    let subtitle: String
    let artworkUrl: String?
    let playing: Bool

    var body: some View {
        HStack(spacing: 12) {
            Artwork(title: title, url: artworkUrl, size: SoundscapeMetrics.rowArtwork)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body).foregroundStyle(.primary).lineLimit(1)
                Text(subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
            if playing {
                Image(systemName: "waveform")
                    .symbolEffect(.variableColor.iterative, options: .repeating)
                    .foregroundStyle(.tint)
                    .accessibilityLabel("Playing")
            }
        }
        .contentShape(Rectangle())
    }
}

extension String {
    /// Nil for an empty string.
    var nonEmpty: String? { isEmpty ? nil : self }
}
#endif
