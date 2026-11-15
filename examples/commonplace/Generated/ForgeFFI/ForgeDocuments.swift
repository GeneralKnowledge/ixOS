// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeDocuments.swift.

// Quick Look's thumbnailer exists only on Apple platforms; on Linux (where
// `swift test` runs an app's models) this file is empty.
#if canImport(QuickLookThumbnailing)
import CoreGraphics
import Foundation
import QuickLookThumbnailing

/// Pictures of documents: the first page of a PDF, a page of a text or
/// office file, a frame of a video, through Quick Look's thumbnailer, the
/// same one Files uses. For opening a document, SwiftUI's own
/// `.quickLookPreview($url)` already shows any file Quick Look can (PDFs
/// with their pages, with sharing), so there is no Forge wrapper for it.
///
/// Quick Look decides the type from the file name, so pass a file whose
/// extension matches its contents (a content-addressed store keeps files
/// without one: link them under their real name first).
///
/// ```swift
/// let page = await ForgeDocuments.thumbnail(of: url, size: CGSize(width: 44, height: 56), scale: displayScale)
/// ```
public enum ForgeDocuments {
    /// A thumbnail of the file at `url` that fits `size` points at `scale`,
    /// or nil when Quick Look can't draw one (an unknown type, a damaged
    /// file). Only the file's own picture counts: never a generic icon.
    public static func thumbnail(of url: URL, size: CGSize, scale: CGFloat) async -> CGImage? {
        let request = QLThumbnailGenerator.Request(
            fileAt: url, size: size, scale: max(1, scale), representationTypes: .thumbnail)
        do {
            return try await QLThumbnailGenerator.shared.generateBestRepresentation(for: request).cgImage
        } catch {
            return nil
        }
    }
}
#endif
