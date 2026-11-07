// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeImages.swift.

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Image thumbnails and downscaling on ImageIO, off the main thread.
///
/// ImageIO decodes straight to the requested size, so a 12 MP photo never
/// sits in memory at full size, and applies the EXIF orientation, so the
/// result is the right way up. Size requests are in pixels: for a view
/// `width` points wide use `ForgeImages.pixels(width, scale: displayScale)`.
///
/// ```swift
/// let image = await ForgeImages.thumbnail(of: url, maxPixels: 440, cached: true)
/// try await ForgeImages.writeDownscaledJPEG(from: data, maxPixels: 2048, to: url)
/// ```
public enum ForgeImages {
    /// Pixels for `points` at a display scale (at least 1).
    public static func pixels(_ points: CGFloat, scale: CGFloat) -> Int {
        max(1, Int((points * scale).rounded(.up)))
    }

    /// A thumbnail of the image file at `url`, at most `maxPixels` on its
    /// longer side, decoded off the main actor. Nil if it is not an image.
    /// With `cached`, the result is kept in memory (dropped under memory
    /// pressure) for the next request of the same file and size, which suits
    /// list rows that scroll in and out.
    public static func thumbnail(of url: URL, maxPixels: Int, cached: Bool = false) async -> CGImage? {
        let key = "\(url.absoluteString)#\(maxPixels)" as NSString
        if cached, let hit = cache.object(forKey: key) { return hit.image }
        let decoded = await Task.detached(priority: .userInitiated) { () -> Decoded? in
            let options = [kCGImageSourceShouldCache: false] as [CFString: Any] as CFDictionary
            guard let source = CGImageSourceCreateWithURL(url as CFURL, options) else { return nil }
            return makeThumbnail(source, maxPixels: maxPixels).map(Decoded.init)
        }.value
        guard let decoded else { return nil }
        if cached { cache.setObject(decoded, forKey: key) }
        return decoded.image
    }

    /// A thumbnail of image `data`, at most `maxPixels` on its longer side,
    /// decoded off the main actor. Nil if it is not an image.
    public static func thumbnail(of data: Data, maxPixels: Int) async -> CGImage? {
        await Task.detached(priority: .userInitiated) { () -> Decoded? in
            guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
            return makeThumbnail(source, maxPixels: maxPixels).map(Decoded.init)
        }.value?.image
    }

    /// Writes image `data` (a photo from the picker, say) to `url` as a JPEG
    /// of at most `maxPixels` on its longer side, the right way up and
    /// without its location or other metadata. Runs off the main actor.
    /// Throws if `data` is not an image or the file cannot be written.
    public static func writeDownscaledJPEG(
        from data: Data, maxPixels: Int, quality: Double = 0.8, to url: URL
    ) async throws {
        try await Task.detached(priority: .userInitiated) {
            guard let source = CGImageSourceCreateWithData(data as CFData, nil),
                let image = makeThumbnail(source, maxPixels: maxPixels)
            else {
                throw CocoaError(.fileReadCorruptFile)
            }
            try writeJPEG(image, quality: quality, to: url)
        }.value
    }

    /// Writes `image` to `url` as a JPEG. A `CGImage` carries no metadata,
    /// so neither does the file.
    public static func writeJPEG(_ image: CGImage, quality: Double = 0.8, to url: URL) throws {
        guard
            let destination = CGImageDestinationCreateWithURL(
                url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)
        else {
            throw CocoaError(.fileWriteUnknown)
        }
        CGImageDestinationAddImage(
            destination, image,
            [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw CocoaError(.fileWriteUnknown)
        }
    }

    // MARK: - Private

    /// A decoded image handed between tasks. A `CGImage` is immutable, so
    /// sharing it is safe.
    private final class Decoded: @unchecked Sendable {
        let image: CGImage
        init(_ image: CGImage) { self.image = image }
    }

    nonisolated(unsafe) private static let cache = NSCache<NSString, Decoded>()

    private static func makeThumbnail(_ source: CGImageSource, maxPixels: Int) -> CGImage? {
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: max(maxPixels, 1),
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
    }
}
