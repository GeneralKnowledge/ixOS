@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// A2UI Lab (A2UI-1): an agent's reading-plan proposal drawn natively from
/// A2UI messages, edited on the surface, and reviewed and confirmed with
/// app-owned controls. Rust checks every message, the review and what
/// Confirm applies (`rust/src/a2ui_lab`).
struct A2uiLabView: View {
    let model: A2uiLabModel
    @StateObject private var host: A2uiSurfaceHost
    @Environment(\.forgeTheme) private var theme

    init(model: A2uiLabModel) {
        self.model = model
        _host = StateObject(wrappedValue: A2uiSurfaceHost(model: model))
    }

    private var reviewing: Binding<Bool> {
        Binding(
            get: { model.changeSet != nil },
            set: { if !$0 { model.cancelReview() } }
        )
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.l) {
                Text("An agent's proposal, drawn natively from A2UI messages. The app checks every message, works out the changes itself, and applies them only when you confirm.")
                    .forgeText(.note)

                if let problem = host.catalogProblem {
                    ForgeNotice(problem, systemImage: "xmark.octagon", tone: .danger)
                }
                if let notice = model.notice {
                    ForgeNotice(notice.text, systemImage: icon(notice.kind), tone: tone(notice.kind))
                        .accessibilityIdentifier("a2uiLab.notice")
                }

                if let surface = host.surface, let catalog = host.catalog {
                    A2uiSurfaceView(surface: surface, catalog: catalog)
                } else {
                    Button("Show the Proposal") { model.start() }
                        .buttonStyle(.forge(.primary, expands: true))
                        .disabled(host.catalog == nil)
                        .accessibilityIdentifier("a2uiLab.start")
                }

                if host.surface != nil {
                    controls
                }
                if !model.saved.isEmpty {
                    saved
                }
            }
            .padding(theme.spacing.l)
        }
        .background(theme.colors.groupedBackground)
        .navigationTitle("A2UI Lab")
        .inlineNavigationTitle()
        .sheet(isPresented: reviewing) {
            if let changeSet = model.changeSet {
                A2uiReviewSheet(
                    changeSet: changeSet,
                    confirm: { model.confirm() },
                    cancel: { model.cancelReview() }
                )
            }
        }
    }

    /// App-owned controls: outside the surface, so an agent can't draw,
    /// hide or press them.
    private var controls: some View {
        VStack(alignment: .leading, spacing: theme.spacing.s) {
            Text("Agent and checks").forgeText(.sectionTitle)
            Button("Send the Agent's Update") { model.agentUpdate() }
                .buttonStyle(.forge(.secondary, expands: true))
                .disabled(model.updated)
                .accessibilityIdentifier("a2uiLab.agentUpdate")
            Menu {
                ForEach(model.hostileCases, id: \.id) { hostile in
                    Button(hostile.title) { model.sendHostile(hostile) }
                        .accessibilityIdentifier("a2uiLab.hostile.\(hostile.id)")
                }
            } label: {
                Text("Send a Bad Payload")
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .accessibilityIdentifier("a2uiLab.hostile")
            Button("Start Over") { model.start() }
                .buttonStyle(.forge(.tertiary, expands: true))
                .accessibilityIdentifier("a2uiLab.startOver")
            Text("A bad payload must be refused with a reason, and the plan above must not change.")
                .forgeText(.note)
        }
    }

    private var saved: some View {
        ForgeCard {
            Text("Saved plans").forgeText(.sectionTitle)
            Text("What Confirm applied, to a stand-in library. No app's books are changed.")
                .forgeText(.note)
            ForEach(model.saved, id: \.title) { plan in
                VStack(alignment: .leading, spacing: theme.spacing.xxs) {
                    Text(plan.title).forgeText(.rowTitle)
                    Text(plan.plan).forgeText(.metadata)
                }
                .accessibilityElement(children: .combine)
            }
        }
        .accessibilityIdentifier("a2uiLab.saved")
    }

    private func icon(_ kind: A2uiLabModel.Notice.Kind) -> String {
        switch kind {
        case .info: "info.circle"
        case .refused: "hand.raised"
        case .warning: "exclamationmark.triangle"
        }
    }

    private func tone(_ kind: A2uiLabModel.Notice.Kind) -> ForgeTone {
        switch kind {
        case .info: .info
        case .refused, .warning: .warning
        }
    }
}

/// The app-owned confirmation. Everything it shows comes from Rust's
/// change set: titles from the app's books, plans from Rust's check of the
/// edited plan, never text the surface wrote.
struct A2uiReviewSheet: View {
    let changeSet: A2uiChangeSet
    let confirm: () -> Void
    let cancel: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section {
                    if changeSet.rows.isEmpty {
                        Text("Nothing would change.").forgeText(.body)
                    }
                    ForEach(changeSet.rows, id: \.bookId) { row in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(row.title).forgeText(.rowTitle)
                            Text(row.author).forgeText(.metadata)
                            Text(row.plan).forgeText(.body)
                            if let was = row.was {
                                Text("Replaces: \(was)").forgeText(.note)
                            }
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("a2uiLab.change.\(row.bookId)")
                    }
                } header: {
                    Text(changeSet.rows.count == 1 ? "1 change" : "\(changeSet.rows.count) changes")
                } footer: {
                    Text(footer)
                }
            }
            .navigationTitle("Review Changes")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: cancel)
                        .accessibilityIdentifier("a2uiLab.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Confirm", action: confirm)
                        .disabled(changeSet.rows.isEmpty)
                        .accessibilityIdentifier("a2uiLab.confirm")
                }
            }
        }
    }

    private var footer: String {
        let same = changeSet.unchanged == 1 ? "1 book stays" : "\(changeSet.unchanged) books stay"
        return "Worked out by the app from your edited plan. \(same) as saved. If the plan changes before you confirm, you'll be asked to review it again."
    }
}
