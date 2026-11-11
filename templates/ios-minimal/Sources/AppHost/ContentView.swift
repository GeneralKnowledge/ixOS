@preconcurrency import FORGE_MODULE_NAMEFFI
@preconcurrency import ForgeKit
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the list once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<AppCore>

    public init(host: ForgeHost<AppCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            ItemsScreen(core: core, changes: host.changes)
        }
    }
}

/// The app's only screen: the list, a field to add to it, tap to tick an
/// item off, swipe to delete. Every control has an accessibility identifier
/// (`screen.element`) so UI tests can find it.
private struct ItemsScreen: View {
    @State private var model: ItemsModel
    @State private var newTitle = ""
    let changes: UInt64

    init(core: AppCore, changes: UInt64) {
        _model = State(initialValue: ItemsModel(core: core))
        self.changes = changes
    }

    var body: some View {
        NavigationStack {
            list
                .navigationTitle("FORGE_APP_NAME")
        }
        // Rust committed a change, or the app came back from the
        // background: show what Rust holds now.
        .onChange(of: changes) { model.reload() }
        .alert(
            "Something Went Wrong",
            isPresented: Binding(
                get: { model.errorMessage != nil },
                set: { if !$0 { model.errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    private var list: some View {
        List {
            Section {
                HStack {
                    TextField("New item", text: $newTitle)
                        .accessibilityIdentifier("items.newTitle")
                        .onSubmit(add)
                    Button("Add", action: add)
                        .accessibilityIdentifier("items.add")
                        .disabled(newTitle.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            Section {
                ForEach(model.items, id: \.id) { item in
                    Button {
                        Task { await model.toggle(item) }
                    } label: {
                        Label(item.title, systemImage: item.done ? "checkmark.circle.fill" : "circle")
                            .strikethrough(item.done)
                    }
                    .foregroundStyle(item.done ? .secondary : .primary)
                    .accessibilityIdentifier("items.row.\(item.id)")
                    .accessibilityValue(item.done ? "Done" : "Not done")
                    .swipeActions {
                        Button("Delete", role: .destructive) {
                            Task { await model.delete(item) }
                        }
                        .accessibilityIdentifier("items.delete.\(item.id)")
                    }
                }
            }
        }
    }

    private func add() {
        let title = newTitle
        newTitle = ""
        Task { await model.add(title: title) }
    }
}
