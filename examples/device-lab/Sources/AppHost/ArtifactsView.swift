@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI
#if canImport(UIKit)
import UIKit
#endif
#if canImport(WebKit) && os(iOS)
import WebKit
#endif
#if canImport(QuickLook) && os(iOS)
import QuickLook
#endif

/// The files a project's scripts made (docs/visual-labs-spec.md, P1, and
/// the artifact browser, PL2e): Python makes the files, this shows them.
/// PNG, JPEG and GIF open in an image view with zoom; SVG and HTML in a web
/// view that can't run scripts or load anything; CSV and TSV as a table and
/// text and JSON as text (both read by Rust, `artifactTable` and
/// `artifactText`); PDFs and anything else in Quick Look.

/// What `ProjectArtifact.kind` holds, beyond the image kinds.
enum ArtifactKind {
    static let images: Set<String> = ["png", "jpeg", "gif", "svg"]
    /// Kinds the Files list opens in the editor rather than a preview.
    static let editable: Set<String> = ["table", "json", "text"]

    static func symbol(_ kind: String) -> String {
        switch kind {
        case "svg": "chart.bar.doc.horizontal"
        case "table": "tablecells"
        case "json": "curlybraces"
        case "text": "doc.plaintext"
        case "html": "globe"
        case "pdf": "doc.richtext"
        case "png", "jpeg", "gif": "photo"
        default: "doc"
        }
    }

    static func message(_ error: Error) -> String {
        if case ArtifactError.Unreadable(let reason) = error { return reason }
        return "This file can't be shown."
    }

    /// The label in a row: "CSV" for a table, the kind otherwise.
    static func label(_ artifact: ProjectArtifact) -> String {
        if artifact.kind == "table" {
            return artifact.path.lowercased().hasSuffix(".tsv") ? "TSV" : "CSV"
        }
        return artifact.kind == "file"
            ? (artifact.path.split(separator: ".").last.map { $0.uppercased() } ?? "FILE")
            : artifact.kind.uppercased()
    }
}

/// One output in a list: a thumbnail or icon, its name and size. Opens it.
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
        var parts = [ArtifactKind.label(artifact), Format.storage(artifact.bytes)]
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
            if artifact.kind == "svg" || !ArtifactKind.images.contains(artifact.kind) {
                ForgeThumbnailPlaceholder(systemImage: ArtifactKind.symbol(artifact.kind))
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

/// An output full screen: an image (zoom with two fingers), a table, text,
/// a web page or Quick Look; share from the toolbar.
struct ArtifactDetailView: View {
    let artifact: ProjectArtifact
    let url: URL?
    @State private var zoom: CGFloat = 1
    @State private var settledZoom: CGFloat = 1

    var body: some View {
        Group {
            switch artifact.kind {
            case "table":
                ArtifactTableView(url: url)
            case "json", "text":
                ArtifactTextView(url: url, kind: artifact.kind)
            case "html":
                ArtifactHTMLView(url: url, name: artifact.path)
            case "png", "jpeg", "gif", "svg":
                image
            default:
                ArtifactQuickLook(url: url, name: artifact.path)
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

    @ViewBuilder private var image: some View {
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
    /// `svg` is a whole HTML page (PL2e) rather than an SVG to inline: the
    /// same policy goes in front of it, and scripts stay off either way.
    var wholePage = false

    func makeCoordinator() -> NavigationGuard { NavigationGuard() }

    /// The page may show itself and nothing else: a link, a meta refresh
    /// or a form can't take the preview to another page or reach the
    /// network.
    @MainActor
    final class NavigationGuard: NSObject, WKNavigationDelegate {
        func webView(
            _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
        ) async -> WKNavigationActionPolicy {
            // loadHTMLString with no base URL loads as about:blank.
            navigationAction.request.url?.absoluteString == "about:blank" ? .allow : .cancel
        }
    }

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.backgroundColor = .clear
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        view.loadHTMLString(wholePage ? Self.policy + svg : Self.page(svg), baseURL: nil)
    }

    /// Ahead of everything else, so the parser puts it in the page's head.
    static let policy = """
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:">
        """

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
    var wholePage = false
    var body: some View {
        ScrollView { Text(svg).font(.system(.caption, design: .monospaced)).padding() }
    }
}
#endif

/// A CSV or TSV file as a grid, its first row as the header.
struct ArtifactTableView: View {
    let url: URL?
    @State private var table: ArtifactTable?
    @State private var failure: String?

    var body: some View {
        Group {
            if let table {
                ScrollView([.horizontal, .vertical]) {
                    VStack(alignment: .leading, spacing: 8) {
                        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 6) {
                            ForEach(Array(table.rows.enumerated()), id: \.offset) { index, row in
                                GridRow {
                                    ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                                        Text(cell)
                                            .font(.system(.footnote, design: .monospaced))
                                            .fontWeight(index == 0 ? .semibold : .regular)
                                            .lineLimit(3)
                                            .frame(maxWidth: 240, alignment: .leading)
                                    }
                                }
                                if index == 0 { Divider() }
                            }
                        }
                        .textSelection(.enabled)
                        if let note = Self.note(table) {
                            Text(note)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("artifacts.table.note")
                        }
                    }
                    .padding()
                }
                .accessibilityIdentifier("artifacts.table")
            } else if let failure {
                Text(failure).foregroundStyle(.secondary).padding()
                    .accessibilityIdentifier("artifacts.unreadable")
            } else {
                ProgressView()
            }
        }
        .task(id: url) {
            guard let url else {
                failure = "This file can't be shown."
                return
            }
            let path = url.path
            do {
                table = try await Task.detached { try artifactTable(file: path) }.value
            } catch {
                failure = ArtifactKind.message(error)
            }
        }
    }

    /// What the grid leaves out, if anything.
    static func note(_ table: ArtifactTable) -> String? {
        var parts: [String] = []
        if table.totalRows > UInt64(table.rows.count) {
            parts.append("Showing the first \(table.rows.count) of \(table.totalRows) rows\(table.partial ? " read" : "").")
        } else if table.partial {
            parts.append("Showing the start of a large file.")
        }
        if table.cut {
            parts.append("Long cells and extra columns are cut; share the file to see it all.")
        }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }
}

/// A text or JSON file, JSON indented, selectable.
struct ArtifactTextView: View {
    let url: URL?
    let kind: String
    @State private var shown: ArtifactText?
    @State private var failure: String?

    var body: some View {
        Group {
            if let shown {
                ScrollView([.vertical, .horizontal]) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(shown.text)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                        if shown.truncated {
                            Text("Showing the start of a large file; share it to see it all.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding()
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .accessibilityIdentifier("artifacts.text")
            } else if let failure {
                Text(failure).foregroundStyle(.secondary).padding()
                    .accessibilityIdentifier("artifacts.unreadable")
            } else {
                ProgressView()
            }
        }
        .task(id: url) {
            guard let url else {
                failure = "This file can't be shown."
                return
            }
            let path = url.path
            do {
                shown = try await Task.detached { try artifactText(file: path) }.value
            } catch {
                failure = ArtifactKind.message(error)
            }
        }
    }
}

/// A PDF or any other file: Quick Look shows what it can.
struct ArtifactQuickLook: View {
    let url: URL?
    let name: String
    @State private var previewing: URL?

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "doc")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(name).font(.headline)
            if let url {
                Button("Open in Quick Look", systemImage: "eye") { previewing = url }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("artifacts.quickLook")
            }
        }
        .padding()
        #if canImport(QuickLook) && os(iOS)
        .quickLookPreview($previewing)
        #endif
    }
}

/// An HTML file a script wrote, read off the main thread and shown in the
/// script-free, network-free web view; one over 2 MB is offered to share
/// instead (PL2e).
struct ArtifactHTMLView: View {
    let url: URL?
    let name: String
    @State private var html: String?
    @State private var failure: String?

    static let maxBytes = 2 * 1024 * 1024

    var body: some View {
        Group {
            if let html {
                SVGView(svg: html, wholePage: true)
                    .accessibilityLabel("Web page \(name)")
                    .accessibilityIdentifier("artifacts.html")
            } else if let failure {
                Text(failure).foregroundStyle(.secondary).padding()
                    .accessibilityIdentifier("artifacts.unreadable")
            } else {
                ProgressView()
            }
        }
        .task(id: url) {
            guard let url else {
                failure = "This file can't be shown."
                return
            }
            let limit = Self.maxBytes
            let read = await Task.detached { () -> Result<String, ArtifactHTMLError> in
                let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
                guard size <= limit else { return .failure(.tooLarge) }
                // Read no more than the limit, even if the file grew since.
                guard let handle = try? FileHandle(forReadingFrom: url) else { return .failure(.unreadable) }
                defer { try? handle.close() }
                let data: Data
                do {
                    // nil at the end: an empty file.
                    data = try handle.read(upToCount: limit + 1) ?? Data()
                } catch {
                    return .failure(.unreadable)
                }
                guard data.count <= limit else { return .failure(.tooLarge) }
                return .success(String(decoding: data, as: UTF8.self))
            }.value
            switch read {
            case .success(let text): html = text
            case .failure(.tooLarge):
                failure = "This page is over 2 MB, too large to preview. Share it to open it elsewhere."
            case .failure(.unreadable): failure = "This file can't be shown."
            }
        }
    }
}

enum ArtifactHTMLError: Error {
    case tooLarge
    case unreadable
}
