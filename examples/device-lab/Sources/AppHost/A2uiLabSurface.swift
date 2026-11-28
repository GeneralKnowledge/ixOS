import A2UICore
import A2UISwiftUI
import Combine
import Foundation
import SwiftUI

// A2UI's official renderer drives each lab surface; every component is
// drawn by the ForgeUI adapter (A2uiForgeCatalog.swift). Device Lab only: no
// other target depends on A2UI
// (docs/research/external-projects/assessments/ai/a2ui.md).

/// Owns A2UI's message processor for one surface. A view-side class
/// because A2UI's core uses Combine; everything it decides goes to the
/// surface's owner (`A2uiSurfaceOwner`), which asks Rust.
@MainActor
final class A2uiSurfaceHost: ObservableObject, A2uiRendering {
    @Published private(set) var surface: SurfaceViewModel?
    let catalog: Catalog<ComponentImplementation>

    private let surfaceId: String
    private weak var model: (any A2uiSurfaceOwner)?
    private var processor: MessageProcessor?
    private var edits: AnyCancellable?

    enum HostError: Error { case noCatalog }

    init(model: any A2uiSurfaceOwner) {
        self.model = model
        surfaceId = model.surfaceId
        catalog = A2uiLabCatalog.make(id: model.catalogId)
        model.renderer = self
    }

    func reset() {
        edits = nil
        processor?.dispose()
        processor = nil
        surface = nil
        // Strict: unknown components, properties and functions, orphans and
        // dangling references are all refused, on top of Rust's gate.
        let processor = MessageProcessor(catalogs: [catalog], validationConfig: .strict)
        let model = self.model
        processor.outboundListener = { [weak model] message in
            Task { @MainActor in
                guard let model else { return }
                switch message {
                case .action(let action):
                    let context = (try? JSONEncoder().encode(action.context))
                        .map { String(decoding: $0, as: UTF8.self) }
                    model.surfaceEvent(
                        name: action.name,
                        sourceComponentId: action.sourceComponentID,
                        contextJson: context ?? "{}")
                case .error(let error):
                    model.rendererError(code: A2uiSurfaceHost.code(error))
                case .callAgentFunction:
                    // The catalog declares no agent functions, so this
                    // shouldn't happen; it is never answered.
                    model.rendererError(code: "AGENT_FUNCTION_REFUSED")
                case .rendererFunctionResponse:
                    break
                }
            }
        }
        self.processor = processor
    }

    func render(messages: [String]) throws {
        if processor == nil { reset() }
        guard let processor else { throw HostError.noCatalog }
        let parser = MessageParser()
        // Parse them all first, so a broken one renders none of them.
        let parsed = try messages.map { try parser.parse(jsonString: $0) }
        processor.process(messages: parsed)
        if surface == nil, let created = processor.surfaceGroupModel.surfacesMap[surfaceId] {
            surface = created
            let model = self.model
            // Every change after the first value is an edit (or the agent's
            // update, which Rust has already accounted for).
            edits = created.dataModel.dataPublisher
                .dropFirst()
                .sink { [weak model] _ in
                    Task { @MainActor in model?.surfaceEdited() }
                }
        }
    }

    nonisolated static func code(_ error: RendererError) -> String {
        switch error {
        case .validationFailed: "VALIDATION_FAILED"
        case .generic(let generic): generic.code
        }
    }
}

/// The surface, drawn by A2UI's renderer with the lab's ForgeUI catalog.
struct A2uiSurfaceView: View {
    @ObservedObject var surface: SurfaceViewModel
    let catalog: Catalog<ComponentImplementation>

    var body: some View {
        A2UISwiftUI.Surface(viewModel: surface, catalogs: [catalog])
    }
}
