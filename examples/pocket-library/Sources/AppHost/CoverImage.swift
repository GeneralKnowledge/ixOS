import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// A book cover: the copy saved on the device if there is one (works
/// offline), else the remote image, else a placeholder with the title.
struct CoverImage: View {
    /// Unused for drawing; kept so callers name the book (and for debugging).
    let title: String
    let file: String?
    let url: String?
    var width: CGFloat = 44

    private var height: CGFloat { width * 1.5 }

    var body: some View {
        Group {
            if let image = LocalCovers.image(at: file) {
                image.resizable().scaledToFill()
            } else if let url, let remote = URL(string: url) {
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

/// Decodes saved cover files once per launch.
@MainActor
enum LocalCovers {
    private static var cache: [String: Image] = [:]

    static func image(at path: String?) -> Image? {
        guard let path else { return nil }
        if let cached = cache[path] { return cached }
        #if canImport(UIKit)
        guard let platform = UIImage(contentsOfFile: path) else { return nil }
        let image = Image(uiImage: platform)
        #elseif canImport(AppKit)
        guard let platform = NSImage(contentsOfFile: path) else { return nil }
        let image = Image(nsImage: platform)
        #endif
        cache[path] = image
        return image
    }
}
