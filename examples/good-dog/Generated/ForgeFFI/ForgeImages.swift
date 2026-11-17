// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeImages.swift.

// ImageIO and CoreGraphics exist only on Apple platforms; on Linux (where
// `swift test` runs an app's models) this file is empty.
#if canImport(ImageIO)
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
/// let cover = await ForgeImages.thumbnail(from: webURL, maxPixels: 440)
/// try await ForgeImages.writeDownscaledJPEG(from: data, maxPixels: 2048, to: url)
/// ```
///
/// In SwiftUI, `ForgeRemoteImage` draws a picture from the web this way,
/// in place of `AsyncImage`. Downloads go through `ForgeWebData`.
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

    /// A thumbnail of the picture at web address `url` (http or https), at
    /// most `maxPixels` on its longer side, decoded off the main actor. Nil
    /// if it can't be fetched or isn't an image.
    ///
    /// The download goes through `ForgeWebData.shared`: HTTP cache headers
    /// decide when the copy on disk is still good, the copy is shown when
    /// there is no connection, responses over 20 MB are refused before they
    /// are decoded, and rows asking for one picture share its download.
    /// Thumbnails stay in memory for up to five minutes; after that the next
    /// request asks `ForgeWebData` again (usually answered from disk) and
    /// decodes again only if the picture changed. With `reload`, the copies
    /// in memory and on disk are skipped and the server asked again.
    public static func thumbnail(from url: URL, maxPixels: Int, reload: Bool = false) async -> CGImage? {
        let key = "\(url.absoluteString)#\(maxPixels)" as NSString
        let held = webCache.object(forKey: key)
        if !reload, let held, isFresh(held, url) { return held.image }
        switch await ForgeWebData.shared.data(from: url, reload: reload) {
        case .success(let data):
            let digest = digestOf(data)
            if let held, held.digest == digest {
                held.storedAt = Date()
                return held.image
            }
            guard !Task.isCancelled, let image = await thumbnail(of: data, maxPixels: maxPixels) else {
                return nil
            }
            webCache.setObject(WebEntry(image, digest: digest), forKey: key)
            return image
        case .failure(.offline):
            // No network and nothing on disk: what is in memory is still
            // the best picture there is.
            return held?.image
        case .failure:
            return nil
        }
    }

    /// The thumbnail `thumbnail(from:maxPixels:)` holds in memory for this
    /// address and size, without waiting; nil if there is none. A view
    /// starts with it so a row scrolled back into sight doesn't blink.
    public static func cachedThumbnail(from url: URL, maxPixels: Int) -> CGImage? {
        guard let held = webCache.object(forKey: "\(url.absoluteString)#\(maxPixels)" as NSString),
            !wasForgotten(held, url)
        else { return nil }
        return held.image
    }

    /// Forgets the picture at `url`: its copy on disk now, and the
    /// thumbnails in memory at their next request.
    public static func forget(_ url: URL) {
        ForgeWebData.shared.forget(url)
        forgetLock.lock()
        forgottenAt[url.absoluteString] = Date()
        forgetLock.unlock()
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

    /// A thumbnail of a web picture, with what it was made from.
    private final class WebEntry: @unchecked Sendable {
        let image: CGImage
        let digest: Int
        var storedAt = Date()
        init(_ image: CGImage, digest: Int) {
            self.image = image
            self.digest = digest
        }
    }

    nonisolated(unsafe) private static let webCache = NSCache<NSString, WebEntry>()
    private static let forgetLock = NSLock()
    nonisolated(unsafe) private static var forgottenAt: [String: Date] = [:]

    /// Young enough to show without asking `ForgeWebData`, and not forgotten.
    private static func isFresh(_ entry: WebEntry, _ url: URL) -> Bool {
        Date().timeIntervalSince(entry.storedAt) < 300 && !wasForgotten(entry, url)
    }

    private static func wasForgotten(_ entry: WebEntry, _ url: URL) -> Bool {
        forgetLock.lock()
        defer { forgetLock.unlock() }
        return forgottenAt[url.absoluteString].map { $0 > entry.storedAt } ?? false
    }

    private static func digestOf(_ data: Data) -> Int {
        var hasher = Hasher()
        data.withUnsafeBytes { hasher.combine(bytes: $0) }
        return hasher.finalize()
    }

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

#if canImport(SwiftUI)
import SwiftUI

/// A picture from the web, decoded at the size it is shown and kept on disk
/// as its HTTP cache headers allow (`ForgeImages.thumbnail(from:maxPixels:)`).
/// Use it in place of `AsyncImage`, which decodes at full size and keeps
/// nothing for offline use.
///
/// ```swift
/// ForgeRemoteImage(url: coverURL, maxPixels: ForgeImages.pixels(66, scale: displayScale)) { image in
///     image.resizable().scaledToFill()
/// } placeholder: { failed in
///     if failed { BookSymbol() } else { ProgressView() }
/// }
/// ```
///
/// `placeholder` is shown while the picture loads (`failed` false) and when
/// there is none to show (`failed` true: no address, not reachable, not an
/// image, too large). A picture already in memory shows at once and is then
/// checked, so one that changed on the server replaces it. Change
/// `refreshID` (on pull to refresh, say) to ask the server again past every
/// cache. The picture is `Image(decorative:)`: label the view yourself, or
/// hide it from VoiceOver if it only decorates.
public struct ForgeRemoteImage<Content: View, Placeholder: View>: View {
    private let url: URL?
    private let maxPixels: Int
    private let refreshID: Int
    private let content: (Image) -> Content
    private let placeholder: (_ failed: Bool) -> Placeholder
    @State private var image: Image?
    @State private var failed = false
    @State private var appliedRefresh: Int

    public init(
        url: URL?,
        maxPixels: Int,
        refreshID: Int = 0,
        @ViewBuilder content: @escaping (Image) -> Content,
        @ViewBuilder placeholder: @escaping (_ failed: Bool) -> Placeholder
    ) {
        self.url = url
        self.maxPixels = maxPixels
        self.refreshID = refreshID
        self.content = content
        self.placeholder = placeholder
        let held = url.flatMap { ForgeImages.cachedThumbnail(from: $0, maxPixels: maxPixels) }
        _image = State(initialValue: held.map { Image(decorative: $0, scale: 1) })
        _appliedRefresh = State(initialValue: refreshID)
    }

    public var body: some View {
        Group {
            if let image {
                content(image)
            } else {
                placeholder(failed)
            }
        }
        .task(id: "\(url?.absoluteString ?? "")#\(maxPixels)#\(refreshID)") {
            guard let url else {
                image = nil
                failed = true
                return
            }
            let reload = refreshID != appliedRefresh
            appliedRefresh = refreshID
            let held = ForgeImages.cachedThumbnail(from: url, maxPixels: maxPixels)
            image = held.map { Image(decorative: $0, scale: 1) }
            failed = false
            let loaded = await ForgeImages.thumbnail(from: url, maxPixels: maxPixels, reload: reload)
            guard !Task.isCancelled else { return }
            if let loaded {
                if held == nil {
                    withAnimation(.easeIn(duration: 0.2)) { image = Image(decorative: loaded, scale: 1) }
                } else if loaded !== held {
                    image = Image(decorative: loaded, scale: 1)
                }
            } else if held == nil {
                failed = true
            }
        }
    }
}
#endif
#endif
