@preconcurrency import PocketLibraryFFI
import SwiftUI

/// A book cover: the copy saved on the device if there is one (works
/// offline), else the remote image, else a placeholder with the title.
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
        Group {
            if let local {
                local.resizable().scaledToFill()
            } else if file == nil, let url, let remote = URL(string: url) {
                AsyncImage(url: remote, transaction: Transaction(animation: .easeIn(duration: 0.2))) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    case .empty:
                        placeholder.overlay { ProgressView().controlSize(.small) }
                    case .failure:
                        placeholder
                    @unknown default:
                        placeholder
                    }
                }
            } else {
                placeholder
            }
        }
        .frame(width: width, height: height)
        .clipShape(RoundedRectangle(cornerRadius: min(width / 12, 6), style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: min(width / 12, 6), style: .continuous)
                .strokeBorder(.quaternary, lineWidth: 0.5)
        }
        .accessibilityHidden(true)
        // Saved covers are decoded off the main thread, at the size shown.
        .task(id: file) {
            guard let file else { local = nil; return }
            local = await CoverThumbnails.image(at: file, maxPixels: Int(height * displayScale))
        }
    }

    /// No cover: a book symbol on a neutral system fill.
    private var placeholder: some View {
        ZStack {
            Rectangle().fill(.fill.secondary)
            Image(systemName: "book.closed")
                .font(width > 60 ? .largeTitle : .body)
                .foregroundStyle(.secondary)
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
