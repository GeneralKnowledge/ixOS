/// The shape of a `ForgeThumbnail`.
public enum ForgeThumbnailShape: Hashable, Sendable {
    /// Rounded corners in proportion to the size: covers, photos, artwork.
    case rounded
    /// People and artists.
    case circle
    /// Square corners, for pictures that sit edge to edge.
    case square
}

/// What a thumbnail's placeholder stands in for.
public enum ForgeThumbnailPhase: Hashable, Sendable {
    /// There is no picture: the symbol on a neutral fill.
    case empty
    /// The picture is on its way: a small spinner on the same fill.
    case loading
    /// The picture couldn't be loaded: drawn like `empty`, because a broken
    /// picture should look calm, not alarming. Say what failed elsewhere if
    /// the person needs to know.
    case failed

    /// The spinner shows only while loading.
    var showsSpinner: Bool { self == .loading }
}

/// The numbers a thumbnail is drawn with, kept apart from SwiftUI so they
/// are tested on Linux.
struct ForgeThumbnailGeometry: Hashable, Sendable {
    var width: Double?
    /// Width over height, as SwiftUI's `aspectRatio` takes it.
    var aspectRatio: Double
    var shape: ForgeThumbnailShape

    init(width: Double?, aspectRatio: Double, shape: ForgeThumbnailShape) {
        self.width = width
        self.aspectRatio = aspectRatio > 0 ? aspectRatio : 1
        self.shape = shape
    }

    /// The height for a fixed width; nil when the thumbnail fills its space.
    var height: Double? { width.map { $0 / aspectRatio } }

    /// Rounded corners grow with the thumbnail (a tenth of its width) up to
    /// the theme's card radius; a thumbnail that fills its space uses the
    /// theme's medium radius.
    func cornerRadius(medium: Double, large: Double) -> Double {
        switch shape {
        case .square, .circle: return 0
        case .rounded:
            guard let width else { return medium }
            return min(width * 0.1, large)
        }
    }

    /// The placeholder's symbol takes about two fifths of the shorter side.
    static func symbolSize(width: Double, height: Double) -> Double {
        max(min(width, height) * 0.4, 1)
    }
}

#if canImport(SwiftUI)
import SwiftUI

/// A frame for any picture: a remote image, a file on the device, a
/// `CGImage`, an app's own drawing. It sizes, shapes and outlines the
/// content, so covers, photos and artwork look the same in every app; the
/// content decides how the picture loads.
///
///     ForgeThumbnail(width: 44, aspectRatio: 2 / 3) {
///         ForgeRemoteImage(url: cover, maxPixels: 132) { $0.resizable().scaledToFill() } placeholder: { failed in
///             ForgeThumbnailPlaceholder(failed ? .failed : .loading, systemImage: "book.closed")
///         }
///     }
///     ForgeThumbnail(width: 40, shape: .circle) { artistImage }
///     ForgeThumbnail(aspectRatio: 3 / 4) { sketchPreview }   // fills the grid cell's width
///
/// - Content fills the frame: give images `.resizable().scaledToFill()`.
/// - While there is no picture, show `ForgeThumbnailPlaceholder` (empty,
///   loading or failed), so placeholders match everywhere. An app whose
///   stand-in is part of its identity (Soundscape's coloured initials) can
///   draw its own.
/// - Hidden from VoiceOver unless given a `label`, since a thumbnail
///   usually sits beside the words that name it. With a label it is one
///   image element.
public struct ForgeThumbnail<Content: View>: View {
    private let geometry: ForgeThumbnailGeometry
    private let label: Text?
    private let content: Content

    @Environment(\.forgeTheme) private var theme

    /// - Parameters:
    ///   - width: the thumbnail's width in points; nil fills the width it is
    ///     offered (a grid cell).
    ///   - aspectRatio: width over height (2/3 for a book cover).
    public init(
        width: CGFloat? = nil,
        aspectRatio: CGFloat = 1,
        shape: ForgeThumbnailShape = .rounded,
        label: Text? = nil,
        @ViewBuilder content: () -> Content
    ) {
        self.geometry = ForgeThumbnailGeometry(width: width.map(Double.init), aspectRatio: Double(aspectRatio), shape: shape)
        self.label = label
        self.content = content()
    }

    public var body: some View {
        let framed = Color.clear
            .aspectRatio(CGFloat(geometry.aspectRatio), contentMode: .fit)
            .frame(width: geometry.width.map { CGFloat($0) }, height: geometry.height.map { CGFloat($0) })
            .overlay { content }
        shaped(framed)
            .modifier(ForgeThumbnailAccessibility(label: label))
    }

    @ViewBuilder
    private func shaped<V: View>(_ view: V) -> some View {
        switch geometry.shape {
        case .circle:
            view.clipShape(Circle())
                .overlay { Circle().strokeBorder(.quaternary, lineWidth: 0.5) }
        case .square:
            view.clipped()
                .overlay { Rectangle().strokeBorder(.quaternary, lineWidth: 0.5) }
        case .rounded:
            let radius = CGFloat(geometry.cornerRadius(medium: theme.radius.m, large: theme.radius.l))
            let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
            view.clipShape(shape)
                .overlay { shape.strokeBorder(.quaternary, lineWidth: 0.5) }
        }
    }
}

private struct ForgeThumbnailAccessibility: ViewModifier {
    let label: Text?

    func body(content: Content) -> some View {
        if let label {
            content
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(label)
                .accessibilityAddTraits(.isImage)
        } else {
            content.accessibilityHidden(true)
        }
    }
}

/// What a `ForgeThumbnail` shows while there is no picture: a symbol on a
/// neutral fill, with a small spinner while loading. The symbol scales with
/// the thumbnail.
public struct ForgeThumbnailPlaceholder: View {
    private let phase: ForgeThumbnailPhase
    private let systemImage: String

    public init(_ phase: ForgeThumbnailPhase = .empty, systemImage: String = "photo") {
        self.phase = phase
        self.systemImage = systemImage
    }

    public var body: some View {
        GeometryReader { proxy in
            ZStack {
                Rectangle().fill(.fill.secondary)
                if phase.showsSpinner {
                    ProgressView().controlSize(.small)
                } else {
                    Image(systemName: systemImage)
                        .font(.system(size: CGFloat(ForgeThumbnailGeometry.symbolSize(
                            width: Double(proxy.size.width), height: Double(proxy.size.height)))))
                        .foregroundStyle(.secondary)
                }
            }
        }
        .accessibilityHidden(true)
    }
}
#endif
