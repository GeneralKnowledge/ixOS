@testable import SketchbookApp
@preconcurrency import SketchbookFFI
import XCTest

/// The model against the real Rust core on a temporary folder. Rust stores
/// drawings without reading them, so any bytes stand in for a PKDrawing.
@MainActor
final class SketchbookModelTests: XCTestCase {
    func testDrawAddLayerAndUndo() async throws {
        let host = makeTemporaryAppHost()
        await host.start()
        let core = try XCTUnwrap(host.core)
        let model = SketchbookModel(core: core)
        XCTAssertTrue(model.sketches.isEmpty)

        let sketchResult = await model.create(paper: .grid)
        let sketch = try XCTUnwrap(sketchResult)
        let id = sketch.summary.id
        XCTAssertEqual(model.sketches.count, 1)
        XCTAssertEqual(sketch.layers.count, 1)

        let layer = try XCTUnwrap(sketch.layers.first).id
        let drawnResult = await model.draw(id, layer: layer, drawing: Data([1, 2, 3]))
        let drawn = try XCTUnwrap(drawnResult)
        XCTAssertEqual(model.drawing(at: drawn.layers.first?.drawingPath), Data([1, 2, 3]))

        // Each stroke is a new file; only the current one stays in memory.
        let redrawnResult = await model.draw(id, layer: layer, drawing: Data([4, 5]))
        let redrawn = try XCTUnwrap(redrawnResult)
        XCTAssertEqual(model.drawing(at: redrawn.layers.first?.drawingPath), Data([4, 5]))
        XCTAssertEqual(model.cachedDrawings, 2)
        model.keepDrawings(of: redrawn)
        XCTAssertEqual(model.cachedDrawings, 1)
        XCTAssertEqual(model.drawing(at: redrawn.layers.first?.drawingPath), Data([4, 5]))

        let twoLayersResult = await model.addLayer(to: id)
        let twoLayers = try XCTUnwrap(twoLayersResult)
        XCTAssertEqual(twoLayers.layers.count, 2)
        XCTAssertNotNil(twoLayers.undoName)

        let undoneResult = await model.undo(id)
        let undone = try XCTUnwrap(undoneResult)
        XCTAssertEqual(undone.layers.count, 1)
        XCTAssertNotNil(undone.redoName)

        // The last layer can't go: the alert says to clear it instead.
        model.askToDeleteLayer(try XCTUnwrap(undone.layers.first), in: id)
        XCTAssertNotNil(model.alert)
        let kept = await model.handle(.deleteLayer(sketch: id, layer: layer))
        XCTAssertNil(kept)
        XCTAssertNotNil(model.alert)
    }

    func testFormatting() {
        XCTAssertEqual(SketchFormat.percent(0.754), "75%")
        XCTAssertEqual(SketchFormat.name(.dots), "Dots")
        XCTAssertEqual(SketchFormat.papers.count, 4)
    }
}
