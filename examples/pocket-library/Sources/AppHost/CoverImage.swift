@preconcurrency import PocketLibraryFFI
import ForgeUI
import SwiftUI

/// A book cover: the copy saved on the device if there is one (works
/// offline), else the remote image (kept on disk by `ForgeRemoteImage`),
/// else ForgeUI's placeholder, all in a `ForgeThumbnail` frame.
struct CoverImage: View {
    /// Not drawn; names the book for callers and debugging.
    let title: String
    let file: String?
    let url: String?
    var width: CGFloat = 44

    @Environment(\.displayScale) private var displayScale
    @State private var local: Image?

    private var height: CGFloat { width * 1.5 }

    var body: some View {
        ForgeThumbnail(width: width, aspectRatio: 2 / 3) {
            if let local {
                local.resizable().scaledToFill()
            } else if file == nil, let url, let remote = URL(string: url) {
                ForgeRemoteImage(url: remote, maxPixels: Int(height * displayScale)) { image in
                    image.resizable().scaledToFill()
                } placeholder: { failed in
                    ForgeThumbnailPlaceholder(failed ? .failed : .loading, systemImage: "book.closed")
                }
            } else {
                ForgeThumbnailPlaceholder(systemImage: "book.closed")
            }
        }
        // Saved covers are decoded off the main thread, at the size shown.
        .task(id: file) {
            guard let file else { local = nil; return }
            local = await CoverThumbnails.image(at: file, maxPixels: Int(height * displayScale))
        }
    }
}

/// Saved cover files, downsampled to the size they are shown at and cached
/// (`ForgeImages`; the cache drops them under memory pressure).
enum CoverThumbnails {
    static func image(at path: String, maxPixels: Int) async -> Image? {
        await ForgeImages.thumbnail(of: URL(fileURLWithPath: path), maxPixels: maxPixels, cached: true)
            .map { Image(decorative: $0, scale: 1) }
    }
}
