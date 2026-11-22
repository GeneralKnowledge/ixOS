@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI
#if canImport(UIKit)
import UIKit
#endif
#if canImport(WebKit) && os(iOS)
import WebKit
#endif

/// The images a project's scripts made (docs/visual-labs-spec.md, P1):
/// Python makes the files, this shows them. PNG, JPEG and GIF open in an
/// image view with zoom; SVG in a web view that can't run scripts or load
/// anything.

/// One image in a list: a thumbnail, its name and size. Opens it.
struct ArtifactRow: View {
    let artifact: ProjectArtifact
    let url: URL?

    var body: some View {
        NavigationLink {
            ArtifactDetailView(artifact: artifact, url: url)
        } label: {
            HStack(spacing: 12) {
                ForgeThumbnail(width: 56) {
                    ArtifactThumbnail(artifact: artifact, url: url)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(artifact.path).font(.body)
                    Text(details).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .accessibilityIdentifier("artifacts.item.\(artifact.path)")
    }

    private var details: String {
        var parts = [artifact.kind.uppercased(), Format.storage(artifact.bytes)]
        if let width = artifact.width, let height = artifact.height {
            parts.append("\(width) × \(height)")
        }
        return parts.joined(separator: " · ")
    }
}

/// A small preview. The file is read each time the row appears, so a run
/// that changed it shows the new picture (rows are keyed by path and time).
struct ArtifactThumbnail: View {
    let artifact: ProjectArtifact
    let url: URL?

    var body: some View {
        Group {
            if artifact.kind == "svg" {
                ForgeThumbnailPlaceholder(systemImage: "chart.bar.doc.horizontal")
            } else if let image = ArtifactImage.load(url) {
                image.resizable().scaledToFill()
            } else {
                ForgeThumbnailPlaceholder(.failed, systemImage: "photo")
            }
        }
        .id("\(artifact.path)-\(artifact.modifiedMs)-\(artifact.bytes)")
        .accessibilityHidden(true)
    }
}

/// An image full screen: zoom with two fingers, share from the toolbar.
struct ArtifactDetailView: View {
    let artifact: ProjectArtifact
    let url: URL?
    @State private var zoom: CGFloat = 1
    @State private var settledZoom: CGFloat = 1

    var body: some View {
        Group {
            if artifact.kind == "svg" {
                if let url, let svg = try? String(contentsOf: url, encoding: .utf8) {
                    SVGView(svg: svg)
                        .accessibilityLabel("SVG image \(artifact.path)")
                        .accessibilityIdentifier("artifacts.svg")
                } else {
                    unreadable
                }
            } else if let image = ArtifactImage.load(url) {
                ScrollView([.horizontal, .vertical]) {
                    image
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity)
                        .scaleEffect(zoom)
                        .padding()
                }
                .gesture(
                    MagnifyGesture()
                        .onChanged { value in zoom = max(1, min(8, settledZoom * value.magnification)) }
                        .onEnded { _ in settledZoom = zoom }
                )
                .onTapGesture(count: 2) {
                    zoom = 1
                    settledZoom = 1
                }
                .accessibilityLabel("Image \(artifact.path)")
                .accessibilityIdentifier("artifacts.image")
            } else {
                unreadable
            }
        }
        .navigationTitle(artifact.path)
        .inlineNavigationTitle()
        .toolbar {
            if let url {
                ShareLink(item: url) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .accessibilityIdentifier("artifacts.share")
            }
        }
    }

    private var unreadable: some View {
        Text("This file can't be shown.")
            .foregroundStyle(.secondary)
            .accessibilityIdentifier("artifacts.unreadable")
    }
}

enum ArtifactImage {
    /// The image at `url`, read from disk now (never a cached copy).
    static func load(_ url: URL?) -> Image? {
        #if canImport(UIKit)
        guard let url, let data = try? Data(contentsOf: url), let image = UIImage(data: data) else {
            return nil
        }
        return Image(uiImage: image)
        #else
        return nil
        #endif
    }
}

#if canImport(WebKit) && os(iOS)
/// An SVG in a web view that can't run scripts or load anything: the SVG is
/// inlined into a page whose Content-Security-Policy allows only inline
/// styles and data: images, JavaScript is off, and the page has no base
/// URL, so a reference to https://… or a <script> in the file does nothing.
struct SVGView: UIViewRepresentable {
    let svg: String

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.backgroundColor = .clear
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        view.loadHTMLString(Self.page(svg), baseURL: nil)
    }

    static func page(_ svg: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>html,body{margin:0;background:#fff}svg{display:block;max-width:100%;height:auto;margin:16px auto}</style>
        </head><body>\(svg)</body></html>
        """
    }
}
#else
struct SVGView: View {
    let svg: String
    var body: some View {
        ScrollView { Text(svg).font(.system(.caption, design: .monospaced)).padding() }
    }
}
#endif
