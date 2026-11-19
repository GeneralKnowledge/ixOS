@preconcurrency import FieldKitFFI
@preconcurrency import ForgeKit
import ForgeUI
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the trips once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<FieldKitCore>

    public init(host: ForgeHost<FieldKitCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            TripsScreen(core: core, host: host, changes: host.changes)
        }
        .forgeTheme(ForgeTheme(accent: .teal))
    }
}

/// Owns the screens' model for as long as the core is open.
private struct TripsScreen: View {
    @State private var model: TripsModel
    let changes: UInt64

    init(core: FieldKitCore, host: ForgeHost<FieldKitCore>, changes: UInt64) {
        let model = TripsModel(core: core)
        model.follow(host)
        _model = State(initialValue: model)
        self.changes = changes
    }

    var body: some View {
        TripListView(model: model)
            // Rust committed a change (a new fix, a note), or the app came
            // back from the background: show what Rust holds now.
            .onChange(of: changes) { model.reload() }
    }
}
