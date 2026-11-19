@preconcurrency import CommonplaceFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the library once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<AppCore>

    public init(host: ForgeHost<AppCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            LibraryScreen(core: core, host: host, changes: host.changes)
        }
        .forgeTheme(ForgeTheme(accent: .indigo))
    }
}

/// Owns the library model for as long as the core is open.
private struct LibraryScreen: View {
    @State private var model: LibraryModel
    let changes: UInt64

    init(core: AppCore, host: ForgeHost<AppCore>, changes: UInt64) {
        let model = LibraryModel(core: core)
        model.follow(host)
        _model = State(initialValue: model)
        self.changes = changes
    }

    var body: some View {
        FeedListView(model: model)
            // Over every screen of the stack, so Undo stays reachable
            // whichever screen the person goes to next.
            .forgeBanner(Bindable(model).banner) { action in
                Task { await model.undo(action) }
            }
            .forgeFeedback(model.feedback)
            // Rust committed a change, or the app came back from the
            // background: show what Rust holds now.
            .onChange(of: changes) { model.reload() }
    }
}
