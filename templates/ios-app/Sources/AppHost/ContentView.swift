@preconcurrency import ForgeFFI
@preconcurrency import ForgeKit
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the notes once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<AppCore>

    public init(host: ForgeHost<AppCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            NotesScreen(core: core, changes: host.changes)
        }
    }
}

/// Owns the screen's model for as long as the core is open.
private struct NotesScreen: View {
    @State private var model: NotesModel
    let changes: UInt64

    init(core: AppCore, changes: UInt64) {
        _model = State(initialValue: NotesModel(core: core))
        self.changes = changes
    }

    var body: some View {
        NoteListView(model: model)
            // Rust committed a change, or the app came back from the
            // background: show what Rust holds now.
            .onChange(of: changes) { model.reload() }
    }
}
