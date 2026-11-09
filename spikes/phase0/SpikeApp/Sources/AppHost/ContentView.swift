#if canImport(SwiftUI)
@preconcurrency import ForgeFFI
import ForgeKit
import SwiftUI

public struct ContentView: View {
    @State private var model: ItemsModel
    @State private var newTitle = ""

    public init(host: ForgeHost<AppCore>) {
        _model = State(initialValue: ItemsModel(host: host))
    }

    public var body: some View {
        NavigationStack {
            List {
                HStack {
                    TextField("New item", text: $newTitle)
                        .accessibilityIdentifier("items.newTitle")
                    Button("Add") {
                        let title = newTitle
                        newTitle = ""
                        Task { await model.add(title: title) }
                    }
                    .accessibilityIdentifier("items.add")
                }
                ForEach(model.items, id: \.id) { item in
                    Button {
                        Task { await model.toggle(item) }
                    } label: {
                        Label(item.title, systemImage: item.done ? "checkmark.circle.fill" : "circle")
                    }
                    .accessibilityIdentifier("items.row.\(item.id)")
                }
            }
            .navigationTitle("Spike")
        }
        .onAppear { model.refresh() }
        .onChange(of: model.host.changes) { model.refresh() }
    }
}
#endif
