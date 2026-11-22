import A2UICore
import A2UISwiftUI
import BasicCatalog
import BasicCatalogSwiftUI
import Combine
import ForgeUI
import Foundation
import SwiftUI

// A2UI-1: the official A2UI SwiftUI renderer, with an ixOS catalog of eight
// components drawn in ForgeUI. Device Lab only: no other target depends on
// A2UI (docs/research/external-projects/assessments/ai/a2ui.md).

/// The lab's catalog: eight components from A2UI's v1.0 basic catalog, no
/// functions (so no `openUrl`, no formatting calls), Text drawn with
/// ForgeUI's text roles. Cards and buttons get ForgeUI looks from
/// `ForgeA2uiCardStyle` and `ForgeA2uiButtonStyle`.
@MainActor
enum A2uiLabCatalog {
    static func make(id: String) throws -> Catalog<ComponentImplementation> {
        try BasicCatalogImplementation.makeCatalog(version: .v10).derive(
            id: id,
            components: [
                "Column", "Row", "Card", "Divider", "Button", "DateTimeInput", "ChoicePicker",
                .add(text),
            ],
            functions: []
        )
    }

    static let text = ComponentImplementation(api: BasicCatalogComponents.v10Text) { node in
        ForgeA2uiText(node: node)
    }
}

/// A2UI's Text in ForgeUI's type: body text, and captions as metadata.
/// Body text leaves its colour to its container, so a button's label takes
/// the button's.
private struct ForgeA2uiText: View {
    let node: Node

    var body: some View {
        let text = node.string(for: "text") ?? ""
        Group {
            if node.string(for: "variant") == "caption" {
                Text(text).forgeText(.metadata)
            } else {
                Text(text).font(.forge(.body))
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .a2uiAccessibleText(text)
    }
}

/// A2UI's Card as a `ForgeCard`.
struct ForgeA2uiCardStyle: A2UICardStyle {
    func makeBody(configuration: Configuration) -> some View {
        ForgeCard { configuration.content }
    }
}

/// A2UI's Button in ForgeUI's colours: primary filled with the accent,
/// others tinted. At least 44 points tall.
struct ForgeA2uiButtonStyle: A2UIButtonStyle {
    @Environment(\.forgeTheme) private var theme

    func makeBody(configuration: Configuration) -> some View {
        let primary = configuration.variant == .primary
        let tone = theme.colors.accent
        configuration.label
            .font(.forge(.rowTitle))
            .foregroundStyle(primary ? tone.onSolid : tone.text)
            .frame(maxWidth: .infinity, minHeight: 44)
            .padding(.horizontal, theme.spacing.m)
            .background(
                primary ? tone.solid : tone.soft,
                in: RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
            )
            .opacity(configuration.isDisabled ? 0.5 : (configuration.isPressed ? 0.8 : 1))
    }
}

/// Owns A2UI's message processor for the lab's one surface. A view-side
/// class because A2UI's core uses Combine; everything it decides goes to
/// `A2uiLabModel`, which asks Rust.
@MainActor
final class A2uiSurfaceHost: ObservableObject, A2uiRendering {
    @Published private(set) var surface: SurfaceViewModel?
    let catalog: Catalog<ComponentImplementation>?
    /// Why the catalog couldn't be built (a bug), if it couldn't.
    let catalogProblem: String?

    private let surfaceId: String
    private weak var model: A2uiLabModel?
    private var processor: MessageProcessor?
    private var edits: AnyCancellable?

    enum HostError: Error { case noCatalog }

    init(model: A2uiLabModel) {
        self.model = model
        surfaceId = model.surfaceId
        do {
            catalog = try A2uiLabCatalog.make(id: model.catalogId)
            catalogProblem = nil
        } catch {
            catalog = nil
            catalogProblem = String(describing: error)
        }
        model.renderer = self
    }

    func reset() {
        edits = nil
        processor?.dispose()
        processor = nil
        surface = nil
        guard let catalog else { return }
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

/// The surface, drawn by A2UI's renderer with the lab's catalog and ForgeUI
/// styles.
struct A2uiSurfaceView: View {
    @ObservedObject var surface: SurfaceViewModel
    let catalog: Catalog<ComponentImplementation>

    var body: some View {
        A2UISwiftUI.Surface(viewModel: surface, catalogs: [catalog])
            .a2uiCardStyle(ForgeA2uiCardStyle())
            .a2uiButtonStyle(ForgeA2uiButtonStyle())
    }
}
