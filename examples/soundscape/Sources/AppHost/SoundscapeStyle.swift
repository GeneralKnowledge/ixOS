#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeUI
import SwiftUI

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

/// The app's motion, each with its Reduce Motion form.
enum SoundscapeMotion {
    /// Artwork easing between playing and paused, and other springy moves.
    static var artworkSpring: Animation { .spring(response: 0.45, dampingFraction: 0.8) }
    /// The background following a new song's colours.
    static var paletteFade: Animation { .easeInOut(duration: 0.6) }

    static func spring(reduced: Bool) -> Animation? { reduced ? nil : artworkSpring }
    static func palette(reduced: Bool) -> Animation? { reduced ? nil : paletteFade }
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
                        NavigationLink(value: seeAll) {
                            Text("See All").font(.subheadline)
                        }
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
                Text(title)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                if let subtitle {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
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

extension PaletteColor {
    var color: Color { Color(red: red, green: green, blue: blue) }
}
#endif
