#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI
import UIKit

// Soundscape's own look (docs/design.md): sizes, motion and the shelves its
// screens are built from. Shared pieces come from ForgeUI; these are the
// app's identity.

/// Sizes and spacing on Soundscape's 4-point scale.
enum SoundscapeMetrics {
    static let rowArtwork: CGFloat = 44
    static let shelfArtwork: CGFloat = 150
    static let screenEdge: CGFloat = 20
    static let shelfGap: CGFloat = 12
    static let sectionGap: CGFloat = 24
    static let nowPlayingEdge: CGFloat = 28
}

/// The app's motion, on ForgeUI's vocabulary (ForgeMotion), each with its
/// Reduce Motion form.
enum SoundscapeMotion {
    /// Artwork easing between playing and paused, and other springy moves.
    static var artworkSpring: Animation { ForgeMotion.emphasised.animation }
    /// The background following a new song's colours.
    static var paletteFade: Animation { ForgeMotion.ambient.animation }

    static func spring(reduced: Bool) -> Animation? { ForgeMotion.emphasised.animation(reduceMotion: reduced) }
    static func palette(reduced: Bool) -> Animation? { ForgeMotion.ambient.animation(reduceMotion: reduced) }
}

extension Font {
    /// Soundscape's own headings: rounded, bold.
    static var shelfTitle: Font { .system(.title2, design: .rounded).weight(.bold) }
}

/// A titled row of things that scrolls sideways, with an optional link to
/// all of them.
struct Shelf<Item: Identifiable, Cell: View>: View {
    let title: LocalizedStringKey
    let items: [Item]
    var seeAll: ListenRoute? = nil
    var identifier: String
    @ViewBuilder let cell: (Item) -> Cell

    var body: some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline) {
                    Text(title)
                        .font(.shelfTitle)
                        .accessibilityAddTraits(.isHeader)
                    Spacer()
                    if let seeAll {
                        // A button, not a NavigationLink: in a List row with
                        // the shelf's tiles, a link would open them all.
                        ForgeSurfaceLink(seeAll) {
                            Text("See All").font(.subheadline).foregroundStyle(.tint)
                        }
                        // "See All Recently Played", not five "See All"s.
                        .accessibilityLabel(Text("See All ") + Text(title))
                        .accessibilityIdentifier("\(identifier).seeAll")
                    }
                }
                .padding(.horizontal, SoundscapeMetrics.screenEdge)
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: SoundscapeMetrics.shelfGap) {
                        ForEach(items) { cell($0) }
                    }
                    .padding(.horizontal, SoundscapeMetrics.screenEdge)
                }
                // forgeBar's room is for vertical lists, not this row.
                .contentMargins(.bottom, 0, for: .scrollContent)
                .scrollClipDisabled()
            }
        }
    }
}

/// An artwork tile with a title and a subtitle under it, for shelves and
/// grids. `size` nil fills the width it is given (a grid column).
struct ArtworkTile: View {
    let title: String
    let subtitle: String?
    let url: String?
    var size: CGFloat? = SoundscapeMetrics.shelfArtwork
    var circle = false
    /// On the phone, coming or failed, beside the title.
    var download: DownloadState = .notDownloaded
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        VStack(alignment: circle ? .center : .leading, spacing: 6) {
            if let size {
                Artwork(title: title, url: url, size: size, circle: circle)
            } else {
                GeometryReader { proxy in
                    Artwork(title: title, url: url, size: proxy.size.width, circle: circle)
                }
                .aspectRatio(1, contentMode: .fit)
            }
            VStack(alignment: circle ? .center : .leading, spacing: 1) {
                HStack(spacing: 4) {
                    Text(title)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(.primary)
                        .lineLimit(typeSize.isAccessibilitySize ? 3 : 1)
                    DownloadBadge(state: download)
                }
                if let subtitle {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(typeSize.isAccessibilitySize ? 2 : 1)
                }
            }
            .multilineTextAlignment(circle ? .center : .leading)
        }
        .frame(width: size)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// Loads the palette of a song's artwork (a tiny thumbnail of it), or its
/// stand-in's colours when it has none or the picture can't be read.
enum ArtworkPaletteLoader {
    static func palette(title: String, url: String?) async -> ArtworkPalette {
        guard let url, let link = URL(string: url),
              let image = await ForgeImages.thumbnail(from: link, maxPixels: 24),
              let palette = ArtworkPalette.from(rgba: rgba(image))
        else { return .standIn(title: title) }
        return palette
    }

    /// The image's pixels as premultiplied RGBA bytes.
    private static func rgba(_ image: CGImage) -> [UInt8] {
        let width = max(image.width, 1)
        let height = max(image.height, 1)
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        let drawn: Bool = bytes.withUnsafeMutableBytes { buffer in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        return drawn ? bytes : []
    }
}

extension View {
    /// The model's Undo banner over this screen. The queue's Undo goes to
    /// the player; the rest to the model.
    func soundscapeBanner(model: ListenModel, player: TakePlayer) -> some View {
        forgeBanner(Bindable(model).banner) { action in
            if case .queue(let index, let position) = action {
                player.putBack(index: index, position: position)
            } else {
                Task { await model.undo(action) }
            }
        }
    }
}

// MARK: Layered Navigation (docs/design.md)

/// The look of surfaces that stack and float.
enum SoundscapeSurface {
    /// An overlay's corners (Up Next, Add to Playlist).
    static let overlayRadius = CGFloat(ForgeRadius.standard.xl)
}

extension View {
    /// An overlay that floats over the surface that opened it: translucent,
    /// so that surface shows through, unless Reduce Transparency is on.
    func soundscapeOverlay() -> some View {
        modifier(SoundscapeOverlay())
    }
}

private struct SoundscapeOverlay: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        content
            .presentationBackground(reduceTransparency
                ? AnyShapeStyle(Color(uiColor: .systemBackground))
                : AnyShapeStyle(.thinMaterial))
            .presentationCornerRadius(SoundscapeSurface.overlayRadius)
    }
}

extension PaletteColor {
    var color: Color { Color(red: red, green: green, blue: blue) }
}
// MARK: - Glass

extension View {
    /// Liquid Glass on iOS 26 for a control that floats over content (the
    /// mini-player); the frosted material it had before on iOS 17 to 25
    /// (ForgeUI's `forgeGlass`).
    func soundscapeGlass<S: Shape>(in shape: S) -> some View {
        forgeGlass(.regular, interactive: true, in: shape)
    }

    /// The round backing of Now Playing's small buttons over the artwork's
    /// colours: clear glass on iOS 26, a faint white disc before. No extra
    /// dim: the background under them is the artwork palette, already
    /// darkened so white keeps 4.5:1 (ArtworkPalette).
    func nowPlayingCircle() -> some View {
        forgeGlass(.clear, interactive: true, in: Circle())
    }
}

#endif
