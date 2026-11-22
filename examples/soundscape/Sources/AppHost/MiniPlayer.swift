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

    let player: TakePlayer
    let zoom: Namespace.ID
    let open: () -> Void

    @Environment(\.miniPlayerInline) private var inline

    var body: some View {
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
            if player.isBuffering { ProgressView().controlSize(.small) }
            controls
        }
        .padding(.leading, inline ? 6 : 10)
        .padding(.trailing, inline ? 8 : 14)
        .modifier(ZoomSource(id: Self.zoomID, namespace: zoom))
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
        if player.isComparing { return current.song }
        return current.artist ?? current.label
    }

    @ViewBuilder private var controls: some View {
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
    /// The mini-player's own card, before iOS 26 gave it the tab bar's
    /// accessory: a rounded material bar just above the tab bar.
    func miniPlayerCard() -> some View {
        self
            .frame(height: 56)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .shadow(color: .black.opacity(0.15), radius: 12, y: 4)
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
