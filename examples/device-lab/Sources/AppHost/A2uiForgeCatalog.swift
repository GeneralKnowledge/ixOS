import A2UICore
import A2UISwiftUI
import BasicCatalog
import ForgeUI
import Foundation
import SwiftUI

// The ForgeUI A2UI adapter: A2UI decides which interface to compose, ForgeUI
// decides how it looks and behaves. Each of the lab's eight components is
// drawn here, from ForgeUI components or from native SwiftUI on ForgeUI's
// tokens; none uses A2UI's own basic-catalog views. Google's `BasicCatalog`
// supplies only the v1.0 component definitions (the schemas the renderer
// validates against). ForgeUI itself knows nothing about A2UI
// (docs/research/external-projects/assessments/ai/a2ui.md, "A2UI and ForgeUI").
//
// Rust's gate (rust/src/a2ui_lab/gate.rs) allows only the properties drawn
// here, so an agent can't ask for a look this adapter doesn't have.

/// The lab's catalog: eight components, no functions (so no `openUrl` and no
/// formatting calls).
@MainActor
enum A2uiLabCatalog {
    static func make(id: String) -> Catalog<ComponentImplementation> {
        Catalog(
            id: id,
            protocolVersion: .v10,
            components: [
                ComponentImplementation(api: BasicCatalogComponents.v10Text) { ForgeA2uiText(node: $0) },
                ComponentImplementation(api: BasicCatalogComponents.v10Column) {
                    ForgeA2uiStack(node: $0, vertical: true)
                },
                ComponentImplementation(api: BasicCatalogComponents.v10Row) {
                    ForgeA2uiStack(node: $0, vertical: false)
                },
                ComponentImplementation(api: BasicCatalogComponents.v10Card) { ForgeA2uiCard(node: $0) },
                ComponentImplementation(api: BasicCatalogComponents.v10Divider) { _ in ForgeA2uiDivider() },
                ComponentImplementation(api: BasicCatalogComponents.v10Button) { ForgeA2uiButton(node: $0) },
                ComponentImplementation(api: BasicCatalogComponents.v10ChoicePicker) {
                    ForgeA2uiChoicePicker(node: $0)
                },
                ComponentImplementation(api: BasicCatalogComponents.v10DateTimeInput) {
                    ForgeA2uiDateInput(node: $0)
                },
            ],
            functions: []
        )
    }
}

/// Text in ForgeUI's type: body text, and captions as metadata. Body text
/// leaves its colour to its container, so a button's label takes the
/// button's.
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

/// Column and Row on ForgeUI's spacing. A row that doesn't fit across (two
/// date fields at a large text size) stacks instead of clipping.
private struct ForgeA2uiStack: View {
    let node: Node
    let vertical: Bool

    @Environment(\.forgeTheme) private var theme
    @Environment(\.a2uiCatalogs) private var catalogs
    @Environment(\.a2uiDefaultCatalogID) private var defaultCatalogID

    var body: some View {
        let children = node.renderedChildren(
            for: "children", in: catalogs, defaultCatalogID: defaultCatalogID)
        Group {
            if vertical {
                column(children, spacing: theme.spacing.m)
            } else {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: theme.spacing.l) {
                        ForEach(children, id: \.id) { child in
                            ComponentNodeView(node: child)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    column(children, spacing: theme.spacing.s)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .a2uiAccessibilityElement(for: node, children: .contain)
    }

    private func column(_ children: [Node], spacing: Double) -> some View {
        VStack(alignment: .leading, spacing: spacing) {
            ForEach(children, id: \.id) { child in
                ComponentNodeView(node: child)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// Card as a `ForgeCard`.
private struct ForgeA2uiCard: View {
    let node: Node

    var body: some View {
        ForgeCard {
            if let child = node.child(for: "child") {
                ComponentNodeView(node: child)
            }
        }
        .a2uiAccessibilityElement(for: node, children: .contain)
    }
}

/// Divider as a hairline in ForgeUI's separator colour.
private struct ForgeA2uiDivider: View {
    @Environment(\.forgeTheme) private var theme
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        Rectangle()
            .fill(theme.colors.separator)
            .frame(maxWidth: .infinity)
            .frame(height: 1 / max(displayScale, 1))
            .accessibilityHidden(true)
    }
}

/// Button in ForgeUI's colours: primary filled with the accent, others
/// tinted, at least 44 points tall. Its action is the one the gate
/// registered; it fires only on a person's tap.
private struct ForgeA2uiButton: View {
    let node: Node

    var body: some View {
        let action = node.action(for: "action", userActivation: true)
        Button {
            action?()
        } label: {
            if let child = node.child(for: "child") {
                ComponentNodeView(node: child)
            }
        }
        .buttonStyle(ForgeA2uiButtonLook(primary: node.string(for: "variant") == "primary"))
        .disabled(!node.isValid || node.bool(for: "disabled") == true)
        .accessibilityIdentifier("a2ui-\(node.id)")
    }
}

private struct ForgeA2uiButtonLook: ButtonStyle {
    let primary: Bool

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
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
            .opacity(isEnabled ? (configuration.isPressed ? 0.8 : 1) : 0.5)
    }
}

/// ChoicePicker as ForgeUI chips: the selected one filled with the accent
/// and ticked (so the state isn't colour alone), the others tinted. Chips
/// wrap to a column when they don't fit across. VoiceOver reads each as a
/// button with its selected state.
private struct ForgeA2uiChoicePicker: View {
    let node: Node

    @Environment(\.forgeTheme) private var theme
    @State private var fallback: [String] = []

    private struct Option: Identifiable {
        let id: String
        let label: String
    }

    private var multiple: Bool {
        (node.string(for: "variant") ?? "mutuallyExclusive") != "mutuallyExclusive"
    }

    private var options: [Option] {
        if let array = node.array(for: "options") {
            return array.enumerated().compactMap { index, element in
                if let plain = element as? String {
                    return Option(id: plain, label: plain)
                }
                guard let dict = element as? ResolvedDictionary else { return nil }
                let label =
                    (dict["label"] as? String) ?? (dict["label"] as? DataBinding<String>)?.value
                    ?? "Option \(index + 1)"
                let value =
                    (dict["value"] as? String) ?? (dict["value"] as? DataBinding<String>)?.value
                    ?? "\(index)"
                return Option(id: value, label: label)
            }
        }
        guard let array = node.jsonValue(for: "options")?.arrayValue else { return [] }
        return array.enumerated().map { index, item in
            if let plain = item.stringValue {
                return Option(id: plain, label: plain)
            }
            return Option(
                id: item["value"]?.stringValue ?? "\(index)",
                label: item["label"]?.stringValue ?? "Option \(index + 1)")
        }
    }

    private var selected: [String] {
        if let many = node.dataBinding(for: "value") as DataBinding<[String]>? {
            return many.value ?? fallback
        }
        if let one = node.dataBinding(for: "value") as DataBinding<String>? {
            if let value = one.value, !value.isEmpty { return [value] }
            return fallback
        }
        if let value = node.string(for: "value"), !value.isEmpty { return [value] }
        return fallback
    }

    private func choose(_ value: String) {
        var now = selected
        if multiple {
            if let index = now.firstIndex(of: value) {
                now.remove(at: index)
            } else {
                now.append(value)
            }
        } else {
            now = [value]
        }
        if let many = node.dataBinding(for: "value") as DataBinding<[String]>? {
            many.set(now)
        } else if let one = node.dataBinding(for: "value") as DataBinding<String>? {
            one.set(now.first ?? "")
        } else {
            fallback = now
        }
    }

    var body: some View {
        let label = node.string(for: "label") ?? ""
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            if !label.isEmpty {
                Text(label).forgeText(.metadata)
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: theme.spacing.s) { chips }
                VStack(alignment: .leading, spacing: theme.spacing.s) { chips }
            }
        }
        .a2uiAccessibilityElement(for: node, children: .contain)
    }

    @ViewBuilder
    private var chips: some View {
        let chosen = selected
        ForEach(options) { option in
            let on = chosen.contains(option.id)
            Button {
                choose(option.id)
            } label: {
                Text(option.label)
                    .font(.forge(.metadata))
            }
            .buttonStyle(.forgeChoice(isSelected: on))
            .accessibilityLabel(option.label)
            .accessibilityIdentifier("a2ui-\(node.id)-\(option.id)")
        }
    }
}

/// DateTimeInput as a native date picker with a ForgeUI label, tinted with
/// the accent. Dates only (`yyyy-MM-dd`, the form Rust checks); the gate
/// refuses time inputs.
private struct ForgeA2uiDateInput: View {
    let node: Node

    @Environment(\.forgeTheme) private var theme
    @State private var fallback = Date()

    private static let day = Date.ISO8601FormatStyle(timeZone: .current)
        .year().month().day().dateSeparator(.dash)

    var body: some View {
        let label = node.string(for: "label") ?? ""
        let date = Binding<Date>(
            get: {
                node.string(for: "value").flatMap { try? Date($0, strategy: Self.day) } ?? fallback
            },
            set: { picked in
                fallback = picked
                if let value = node.dataBinding(for: "value") as DataBinding<String>? {
                    value.set(picked.formatted(Self.day))
                }
            }
        )
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            if !label.isEmpty {
                Text(label)
                    .forgeText(.metadata)
                    .accessibilityHidden(true)
            }
            DatePicker(label, selection: date, displayedComponents: .date)
                .labelsHidden()
                .tint(theme.colors.accent.solid)
                .accessibilityIdentifier("a2ui-\(node.id)")
        }
    }
}
