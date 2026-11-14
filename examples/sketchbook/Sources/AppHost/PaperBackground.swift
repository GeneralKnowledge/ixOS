@preconcurrency import SketchbookFFI
import CoreGraphics
import SwiftUI

/// Where the lines, squares or dots of a paper go, in points. Shared by the
/// screen (SwiftUI `Canvas`) and exports (Core Graphics), so both match.
enum PaperPattern {
    static let spacing: CGFloat = 32
    static let dotRadius: CGFloat = 1.5

    /// The paper's own colour: warm white, so ink in any colour shows.
    static let color = CGColor(red: 0.99, green: 0.98, blue: 0.95, alpha: 1)
    static let lineColor = CGColor(red: 0.55, green: 0.65, blue: 0.80, alpha: 0.45)

    /// The path of the pattern within `rect`, or `nil` for plain paper.
    /// Lines are stroked; dots are filled.
    static func path(_ paper: Paper, in rect: CGRect) -> CGPath? {
        let path = CGMutablePath()
        switch paper {
        case .plain:
            return nil
        case .lined:
            for y in stride(from: rect.minY + spacing * 2, to: rect.maxY, by: spacing) {
                path.move(to: CGPoint(x: rect.minX, y: y))
                path.addLine(to: CGPoint(x: rect.maxX, y: y))
            }
        case .grid:
            for y in stride(from: rect.minY + spacing, to: rect.maxY, by: spacing) {
                path.move(to: CGPoint(x: rect.minX, y: y))
                path.addLine(to: CGPoint(x: rect.maxX, y: y))
            }
            for x in stride(from: rect.minX + spacing, to: rect.maxX, by: spacing) {
                path.move(to: CGPoint(x: x, y: rect.minY))
                path.addLine(to: CGPoint(x: x, y: rect.maxY))
            }
        case .dots:
            for y in stride(from: rect.minY + spacing, to: rect.maxY, by: spacing) {
                for x in stride(from: rect.minX + spacing, to: rect.maxX, by: spacing) {
                    path.addEllipse(in: CGRect(
                        x: x - dotRadius, y: y - dotRadius,
                        width: dotRadius * 2, height: dotRadius * 2))
                }
            }
        }
        return path
    }

    /// Draws the paper and its pattern into a Core Graphics context.
    static func draw(_ paper: Paper, in rect: CGRect, context: CGContext) {
        context.setFillColor(color)
        context.fill(rect)
        guard let pattern = path(paper, in: rect) else { return }
        context.addPath(pattern)
        if paper == .dots {
            context.setFillColor(lineColor)
            context.fillPath()
        } else {
            context.setStrokeColor(lineColor)
            context.setLineWidth(1)
            context.strokePath()
        }
    }
}

/// The paper on screen.
struct PaperBackground: View {
    let paper: Paper

    var body: some View {
        Canvas { context, size in
            let rect = CGRect(origin: .zero, size: size)
            context.fill(Path(rect), with: .color(Color(cgColor: PaperPattern.color)))
            guard let pattern = PaperPattern.path(paper, in: rect) else { return }
            let path = Path(pattern)
            let ink = Color(cgColor: PaperPattern.lineColor)
            if paper == .dots {
                context.fill(path, with: .color(ink))
            } else {
                context.stroke(path, with: .color(ink), lineWidth: 1)
            }
        }
        .accessibilityHidden(true)
    }
}
