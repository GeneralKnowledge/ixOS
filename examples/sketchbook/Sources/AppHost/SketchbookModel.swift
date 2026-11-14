@preconcurrency import SketchbookFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

private let log = ForgeLog("Sketchbook")

/// What the screens show and the actions they take. It holds no rules:
/// layers, undo and what an export contains are Sketchbook's Rust core
/// (`rust/src/lib.rs`); this model asks it and shows what it says.
/// Foundation and Observation only, so it compiles and tests on Linux.
@Observable
@MainActor
public final class SketchbookModel {
    /// Every sketch, most recently changed first, as Rust last reported it.
    public private(set) var sketches: [SketchSummary] = []

    /// Alerts the screens ask for: delete after confirming, or a failure.
    public enum Action: Equatable, Sendable {
        case deleteSketch(UInt64)
        case deleteLayer(sketch: UInt64, layer: UInt64)
    }

    public var alert: ForgeAlert<Action>?
    /// Increases after each saved change; views use it as a haptic trigger.
    public private(set) var saveCount = 0
    /// Increases after each saved thumbnail. A thumbnail keeps its file
    /// name, so the grid uses this to know the picture changed.
    public private(set) var thumbnailsSaved = 0

    public let core: SketchbookCore
    /// Drawing bytes by file path. A drawing's file is named by its
    /// contents and never changes, so a path always means the same bytes.
    private var drawings: [String: Data] = [:]

    public init(core: SketchbookCore) {
        self.core = core
        reload()
    }

    /// Re-reads Rust's published sketches. Cheap: no command is waited for.
    public func reload() {
        sketches = core.sketches()
    }

    /// A sketch with its layers and undo state, or `nil` once deleted.
    public func detail(_ id: UInt64) -> SketchDetail? {
        try? core.sketch(id: id)
    }

    /// A layer's PencilKit drawing as bytes, or `nil` while it is empty.
    public func drawing(at path: String?) -> Data? {
        guard let path else { return nil }
        if let cached = drawings[path] { return cached }
        let data = try? Data(contentsOf: URL(fileURLWithPath: path))
        drawings[path] = data
        return data
    }

    // MARK: Sketches

    /// A new sketch with one empty layer; `nil` (and an alert) if it failed.
    public func create(paper: Paper = .plain, now: Date = Date()) async -> SketchDetail? {
        await perform { core in
            try await core.createSketch(title: SketchFormat.defaultTitle(now: now), paper: paper)
        }
    }

    public func duplicate(_ sketch: SketchSummary) async {
        _ = await perform { core in try await core.duplicateSketch(id: sketch.id) }
    }

    /// Renames; throws so the rename field can show the problem itself.
    public func rename(_ id: UInt64, to title: String) async throws {
        try await core.renameSketch(id: id, title: title)
        reload()
    }

    public func askToDelete(_ sketch: SketchSummary) {
        alert = .confirm(
            "Delete “\(sketch.title)”?",
            message: "Its layers and drawings are deleted too. This can't be undone.",
            confirm: "Delete Sketch",
            action: .deleteSketch(sketch.id),
            identifier: "sketches.confirmDelete",
            destructive: true
        )
    }

    public func askToDeleteLayer(_ layer: Layer, in sketch: UInt64) {
        alert = .confirm(
            "Delete “\(layer.name)”?",
            message: "Undo brings it back.",
            confirm: "Delete Layer",
            action: .deleteLayer(sketch: sketch, layer: layer.id),
            identifier: "layers.confirmDelete",
            destructive: true
        )
    }

    /// Runs a confirmed action; returns the sketch after a layer change.
    @discardableResult
    public func handle(_ action: Action) async -> SketchDetail? {
        switch action {
        case .deleteSketch(let id):
            _ = await perform { core in try await core.deleteSketch(id: id) }
            return nil
        case .deleteLayer(let sketch, let layer):
            return await perform { core in try await core.deleteLayer(sketchId: sketch, layerId: layer) }
        }
    }

    // MARK: Drawing and layers (each one undo step in Rust)

    /// A layer's drawing after a stroke ended.
    public func draw(_ sketch: UInt64, layer: UInt64, drawing: Data) async -> SketchDetail? {
        await perform(haptic: false) { core in
            try await core.draw(sketchId: sketch, layerId: layer, drawing: drawing)
        }
    }

    public func clear(_ sketch: UInt64, layer: UInt64) async -> SketchDetail? {
        await perform { core in try await core.clearLayer(sketchId: sketch, layerId: layer) }
    }

    public func addLayer(to sketch: UInt64) async -> SketchDetail? {
        await perform { core in try await core.addLayer(sketchId: sketch, name: nil) }
    }

    public func duplicateLayer(_ sketch: UInt64, layer: UInt64) async -> SketchDetail? {
        await perform { core in try await core.duplicateLayer(sketchId: sketch, layerId: layer) }
    }

    /// Moves a layer to `index`, counted from the bottom.
    public func moveLayer(_ sketch: UInt64, layer: UInt64, to index: Int) async -> SketchDetail? {
        await perform { core in
            try await core.moveLayer(sketchId: sketch, layerId: layer, to: UInt32(max(0, index)))
        }
    }

    public func renameLayer(_ sketch: UInt64, layer: UInt64, to name: String) async -> SketchDetail? {
        await perform { core in try await core.renameLayer(sketchId: sketch, layerId: layer, name: name) }
    }

    public func setVisible(_ sketch: UInt64, layer: UInt64, _ visible: Bool) async -> SketchDetail? {
        await perform(haptic: false) { core in
            try await core.setLayerVisible(sketchId: sketch, layerId: layer, visible: visible)
        }
    }

    public func setOpacity(_ sketch: UInt64, layer: UInt64, _ opacity: Double) async -> SketchDetail? {
        await perform(haptic: false) { core in
            try await core.setLayerOpacity(sketchId: sketch, layerId: layer, opacity: min(1, max(0, opacity)))
        }
    }

    public func setPaper(_ sketch: UInt64, _ paper: Paper) async -> SketchDetail? {
        await perform { core in try await core.setPaper(sketchId: sketch, paper: paper) }
    }

    public func undo(_ sketch: UInt64) async -> SketchDetail? {
        await perform(haptic: false) { core in try await core.undo(sketchId: sketch) }
    }

    public func redo(_ sketch: UInt64) async -> SketchDetail? {
        await perform(haptic: false) { core in try await core.redo(sketchId: sketch) }
    }

    /// The list's picture of a sketch, rendered by the view after a change.
    public func setThumbnail(_ sketch: UInt64, png: Data) async {
        do {
            try await core.setThumbnail(sketchId: sketch, png: png)
            thumbnailsSaved += 1
            reload()
        } catch {
            // A missing thumbnail only costs a picture in the list.
            log.warn("thumbnail not saved: \(error)")
        }
    }

    /// What to render for an export, or `nil` (and an alert).
    public func exportPlan(_ ids: [UInt64], format: ExportFormat) -> ExportPlan? {
        do {
            return try core.exportPlan(sketchIds: ids, format: format)
        } catch {
            alert = .failure(error)
            return nil
        }
    }

    /// Reports a failure the views ran into themselves (rendering).
    public func report(_ error: Error) {
        log.warn("failed: \(error)")
        alert = .failure(error)
    }

    private func perform<T>(
        haptic: Bool = true,
        _ command: (SketchbookCore) async throws -> T
    ) async -> T? {
        do {
            let value = try await command(core)
            reload()
            if haptic { saveCount += 1 }
            return value
        } catch {
            log.warn("command failed: \(error)")
            alert = .failure(error)
            return nil
        }
    }

    // MARK: Words for the screen

    nonisolated public static func describe(_ error: SketchbookError) -> String {
        switch error {
        case .EmptyName: return "Give it a name."
        case .NameTooLong(let max): return "Names can be up to \(max) characters."
        case .NotFound: return "That sketch or layer no longer exists."
        case .TooManyLayers(let max): return "A sketch can have up to \(max) layers."
        case .LastLayer: return "A sketch keeps at least one layer. Clear it instead."
        case .BadOpacity: return "Opacity goes from 0 to 100%."
        case .NothingToUndo: return "There's nothing to undo."
        case .NothingToRedo: return "There's nothing to redo."
        case .NothingToExport: return "Choose a sketch to export."
        case .Drawing: return "The drawing couldn't be saved."
        case .InUse: return "Sketchbook is open in another copy of the app."
        case .Corrupt: return "Your sketches couldn't be read."
        case .NewerData: return "Your sketches were saved by a newer version of Sketchbook."
        case .Storage: return "Couldn't save. Try again."
        case .Stopped: return "Stopped."
        }
    }

    nonisolated public static func title(_ error: SketchbookError) -> String {
        switch error {
        case .EmptyName, .NameTooLong, .BadOpacity: return "Check the Details"
        case .NotFound: return "Not Found"
        case .TooManyLayers, .LastLayer: return "Can't Change Layers"
        case .NothingToUndo, .NothingToRedo, .NothingToExport: return "Nothing to Do"
        case .InUse: return "Already Open"
        case .Drawing, .Corrupt, .NewerData, .Storage: return "Couldn't Save"
        case .Stopped: return "Stopped"
        }
    }
}

extension SketchbookError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Storage, .Drawing: retryable = true
        default: retryable = false
        }
        return ForgeFailure(
            title: SketchbookModel.title(self),
            message: SketchbookModel.describe(self),
            isRetryable: retryable
        )
    }

    public var isCancellation: Bool {
        if case .Stopped = self { return true }
        return false
    }
}

extension SketchSummary: Identifiable {}
extension Layer: Identifiable {}

// MARK: Formatting (Foundation only, so it tests on Linux)

public enum SketchFormat {
    /// The name a new sketch starts with: "Sketch, 14 Nov".
    public static func defaultTitle(now: Date = Date()) -> String {
        "Sketch, \(now.formatted(.dateTime.day().month(.abbreviated)))"
    }

    public static func date(_ ms: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
    }

    /// "75%".
    public static func percent(_ opacity: Double) -> String {
        "\(Int((opacity * 100).rounded()))%"
    }

    public static func name(_ paper: Paper) -> String {
        switch paper {
        case .plain: return "Plain"
        case .lined: return "Lined"
        case .grid: return "Grid"
        case .dots: return "Dots"
        }
    }

    /// Every paper, in menu order.
    public static let papers: [Paper] = [.plain, .lined, .grid, .dots]
}
