@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import ForgeUI
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, your music once it has, or a recovery screen if it failed.
/// Soundscape opens straight on the library, with the player under every
/// screen (ListenView.swift).
public struct ContentView: View {
    @Bindable private var host: ForgeHost<SoundscapeCore>

    public init(host: ForgeHost<SoundscapeCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            #if os(iOS)
            ListenView(core: core, host: host)
            #else
            Text("Soundscape plays music on an iPhone.")
            #endif
        }
        .forgeTheme(.standard)
    }
}
