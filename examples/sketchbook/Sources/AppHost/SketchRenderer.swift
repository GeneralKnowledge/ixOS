#if os(iOS)
@preconcurrency import SketchbookFFI
import PencilKit
import UIKit

/// Draws sketches for export and for the list's thumbnails, from what
/// Rust says goes on each page (`export_plan`). PencilKit renders the ink;
/// the page is the drawing's bounds with a margin, on its paper.
enum SketchRenderer {
    /// A page with nothing drawn yet.
    static let emptyPage = CGRect(x: 0, y: 0, width: 600, height: 800)
    static let margin: CGFloat = 32

    struct Ink {
        let drawing: PKDrawing
        let opacity: CGFloat
    }

    static func ink(_ layers: [ExportLayer]) -> [Ink] {
        layers.compactMap { layer in
            guard
                let data = try? Data(contentsOf: URL(fileURLWithPath: layer.drawingPath)),
                let drawing = try? PKDrawing(data: data)
            else { return nil }
            return Ink(drawing: drawing, opacity: CGFloat(layer.opacity))
        }
    }

    /// The area worth exporting: every stroke, plus a margin.
    static func bounds(_ ink: [Ink]) -> CGRect {
        let drawn = ink.map(\.drawing.bounds).filter { !$0.isEmpty }
        guard let first = drawn.first else { return emptyPage }
        return drawn.dropFirst().reduce(first) { $0.union($1) }
            .insetBy(dx: -margin, dy: -margin)
            .integral
    }

    /// Paints a page into the current context: paper, then each layer,
    /// bottom first. Ink renders in light-mode colours, as on paper.
    static func paint(_ ink: [Ink], paper: Paper, in rect: CGRect, context: CGContext, scale: CGFloat) {
        PaperPattern.draw(paper, in: rect, context: context)
        UITraitCollection(userInterfaceStyle: .light).performAsCurrent {
            for layer in ink {
                layer.drawing.image(from: rect, scale: scale)
                    .draw(in: rect, blendMode: .normal, alpha: layer.opacity)
            }
        }
    }

    static func image(_ page: ExportPage, scale: CGFloat = 2) -> UIImage {
        let ink = ink(page.layers)
        let rect = bounds(ink)
        let format = UIGraphicsImageRendererFormat()
        format.scale = scale
        return UIGraphicsImageRenderer(bounds: rect, format: format).image { context in
            paint(ink, paper: page.paper, in: rect, context: context.cgContext, scale: scale)
        }
    }

    /// Writes the plan's files; returns the first, for the preview.
    static func write(_ plan: ExportPlan) throws -> URL {
        let urls = plan.paths.map { URL(fileURLWithPath: $0) }
        guard let first = urls.first else { throw CocoaError(.fileWriteUnknown) }
        switch plan.format {
        case .png:
            for (page, url) in zip(plan.pages, urls) {
                guard let data = image(page).pngData() else { throw CocoaError(.fileWriteUnknown) }
                try data.write(to: url, options: .atomic)
            }
        case .pdf:
            let pages = plan.pages.map { page in (page, ink(page.layers)) }
            let firstRect = pages.first.map { bounds($0.1) } ?? emptyPage
            let renderer = UIGraphicsPDFRenderer(bounds: firstRect)
            try renderer.writePDF(to: first) { context in
                for (page, ink) in pages {
                    let rect = bounds(ink)
                    context.beginPage(withBounds: rect, pageInfo: [:])
                    // PDF is vector: the scale only matters for PencilKit's
                    // rasterised ink.
                    paint(ink, paper: page.paper, in: rect, context: context.cgContext, scale: 2)
                }
            }
        }
        return first
    }

    /// A small PNG of a sketch for the list.
    static func thumbnail(_ detail: SketchDetail) -> Data? {
        let page = ExportPage(
            sketchId: detail.summary.id,
            title: detail.summary.title,
            paper: detail.summary.paper,
            layers: detail.layers.compactMap { layer in
                guard layer.visible, layer.opacity > 0, let path = layer.drawingPath else { return nil }
                return ExportLayer(drawingPath: path, opacity: layer.opacity)
            }
        )
        let full = bounds(ink(page.layers))
        let scale = min(1, 300 / max(full.width, full.height))
        return image(page, scale: scale).pngData()
    }
}
#endif
