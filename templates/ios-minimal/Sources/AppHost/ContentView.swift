@preconcurrency import ForgeFFI
import SwiftUI

/// The app's only screen: the list, a field to add to it, tap to tick an
/// item off, swipe to delete. Every control has an accessibility identifier
/// (`screen.element`) so UI tests can find it.
public struct ContentView: View {
    @State private var model = ItemsModel()
    @State private var newTitle = ""

    public init() {}

    public var body: some View {
        NavigationStack {
            Group {
                switch model.phase {
                case .loading:
                    ProgressView()
                case .ready:
                    list
                case .failed(let message):
                    ContentUnavailableView {
                        Label("Can't Open the List", systemImage: "exclamationmark.triangle")
                    } description: {
                        Text(message)
                    } actions: {
                        Button("Try Again") { Task { await model.open() } }
                            .accessibilityIdentifier("items.retry")
                    }
                }
            }
            .navigationTitle("FORGE_APP_NAME")
        }
        .task { await model.open() }
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
