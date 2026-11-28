#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeUI
import SwiftUI

private struct MiniPlayerInlineKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// The mini-player sits inside the minimised tab bar (iOS 26): show
    /// less.
    var miniPlayerInline: Bool {
        get { self[MiniPlayerInlineKey.self] }
        set { self[MiniPlayerInlineKey.self] = newValue }
    }
}

/// What's playing, above the tab bar on every screen; tap it for Now
/// Playing. With nothing playing it says so, so it never jumps in and out.
struct MiniPlayer: View {
    static let zoomID = "soundscape.nowPlaying"

    let model: ListenModel
    let player: TakePlayer
    /// Live radio, shown instead of the music while a station is tuned in.
    let live: LivePlayer
    let zoom: Namespace.ID
    let open: () -> Void

    @Environment(\.miniPlayerInline) private var inline

    var body: some View {
        Group {
            if let station = live.station {
                liveBar(station)
            } else {
                musicBar
            }
        }
        .padding(.leading, inline ? 6 : 10)
        .padding(.trailing, inline ? 8 : 14)
        // A bar of fixed height, like the tab bar under it.
        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
        .modifier(ZoomSource(id: Self.zoomID, namespace: zoom))
    }

    /// A live station: its name, LIVE (or why it isn't playing), play or
    /// stop, and end live radio.
    private func liveBar(_ station: LiveStation) -> some View {
        HStack(spacing: 10) {
            Button(action: open) {
                HStack(spacing: 10) {
                    Artwork(title: station.name, url: station.artworkUrl, size: inline ? 28 : 36)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(station.name)
                            .font(.subheadline.weight(.semibold))
                            .lineLimit(1)
                        if !inline {
                            if let failure = live.failure {
                                Text(failure).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                            } else {
                                LiveBadge()
                            }
                        }
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Live Radio: \(station.name)")
            .accessibilityHint("Opens the station")
            .accessibilityIdentifier("nowPlaying.open")
            if live.isConnecting {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel("Connecting")
            }
            Button { live.toggle() } label: {
                Image(systemName: live.isPlaying ? "stop.fill" : "play.fill")
                    .font(.title3)
                    .contentTransition(.symbolEffect(.replace))
                    .frame(width: 36, height: 36)
                    .contentShape(Rectangle())
            }
            .accessibilityLabel(live.isPlaying ? "Stop" : "Play Live")
            .accessibilityIdentifier("nowPlaying.playPause")
            if !inline {
                Button { live.stop() } label: {
                    Image(systemName: "xmark")
                        .font(.body.bold())
                        .frame(width: 36, height: 36)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("Stop Live Radio")
                .accessibilityIdentifier("nowPlaying.endLive")
            }
        }
    }

    private var musicBar: some View {
        HStack(spacing: 10) {
            Button(action: open) {
                HStack(spacing: 10) {
                    artwork
                    titles
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(player.current == nil)
            .accessibilityLabel(player.current.map { "Now Playing: \($0.song)" } ?? "Not Playing")
            .accessibilityHint(player.current == nil ? "" : "Opens the player")
            .accessibilityIdentifier("nowPlaying.open")
            if player.isBuffering || player.switching != nil {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel(player.switching.map { "Loading \($0.song)" } ?? "Loading")
            }
            controls
        }
    }

    @ViewBuilder private var artwork: some View {
        let side: CGFloat = inline ? 28 : 36
        if let current = player.current {
            Artwork(title: current.song, url: current.artworkUrl, size: side)
                .id(current.id)
                .transition(.opacity)
        } else {
            ForgeThumbnail(width: side) {
                ForgeThumbnailPlaceholder(.empty, systemImage: "music.note")
            }
        }
    }

    @ViewBuilder private var titles: some View {
        if let current = player.current {
            VStack(alignment: .leading, spacing: 0) {
                Text(player.isComparing ? "Comparing · \(player.active == .a ? "A" : "B")" : current.song)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                if !inline, let line = subtitle(current) {
                    Text(line)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        } else {
            Text("Not Playing")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)
        }
    }

    private func subtitle(_ current: TakePlayer.Loaded) -> String? {
        if let failure = player.failure { return failure }
        if let next = player.switching { return "Loading \(next.song)…" }
        if player.isComparing { return current.song }
        return current.artist ?? current.label
    }

    @ViewBuilder private var controls: some View {
        // A heart for the song playing, without opening Now Playing.
        if !inline, let trackId = player.current?.trackId, !player.isComparing {
            let on = model.isFavourite(trackId)
            Button {
                Task {
                    if let queued = player.currentQueued, queued.trackId == trackId {
                        await model.toggleFavourite(queued: queued)
                    } else {
                        await model.toggleFavourite(trackId)
                    }
                }
            } label: {
                Image(systemName: on ? "heart.fill" : "heart")
                    .font(.body)
                    .foregroundStyle(on ? Color.pink : Color.primary)
                    .contentTransition(.symbolEffect(.replace))
                    .frame(width: 36, height: 36)
                    .contentShape(Rectangle())
            }
            .accessibilityLabel(on ? "Remove from Favourites" : "Add to Favourites")
            .accessibilityIdentifier("nowPlaying.favourite")
        }
        Button { player.toggle() } label: {
            Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                .font(.title3)
                .contentTransition(.symbolEffect(.replace))
                .frame(width: 36, height: 36)
                .contentShape(Rectangle())
        }
        .disabled(player.current == nil)
        .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
        .accessibilityIdentifier("nowPlaying.playPause")
        if !inline {
            if player.isComparing {
                Button { player.stop() } label: {
                    Image(systemName: "xmark")
                        .font(.body.bold())
                        .frame(width: 36, height: 36)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("Stop")
                .accessibilityIdentifier("nowPlaying.stop")
            } else {
                Button { player.next() } label: {
                    Image(systemName: "forward.fill")
                        .font(.title3)
                        .frame(width: 36, height: 36)
                        .contentShape(Rectangle())
                }
                .disabled(!player.hasNext)
                .accessibilityLabel("Next")
                .accessibilityIdentifier("nowPlaying.next")
            }
        }
    }
}

extension View {
    /// The mini-player's card just above the tab bar: Liquid Glass on
    /// iOS 26, a rounded material bar before.
    func miniPlayerCard() -> some View {
        self
            .frame(height: 56)
            .soundscapeGlass(in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .padding(.horizontal, 8)
            .padding(.bottom, 6)
    }
}

/// Marks the mini-player as where Now Playing zooms out of (iOS 18 and
/// later).
private struct ZoomSource: ViewModifier {
    let id: String
    let namespace: Namespace.ID

    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 18.0, *) {
            content.matchedTransitionSource(id: id, in: namespace)
        } else {
            content
        }
    }
}
#endif
