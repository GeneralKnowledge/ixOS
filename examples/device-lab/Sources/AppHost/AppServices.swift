@preconcurrency import ForgeFFI
import SwiftUI

/// Forge's native services for the Rust core: the core set (Keychain,
/// directories, notifications, permissions, deep links) plus the optional
/// services `rust/src/lib.rs` selects (connectivity, device status, motion,
/// location, Bluetooth). See docs/native-services.md.
///
/// The app's entry point calls `AppServices.install()` once, from
/// `App.init()`, so Core Location and the device status monitor are created
/// on the main thread.
public enum AppServices {
    @MainActor
    public static func install() {
        ForgeServices.install()
    }
}

extension View {
    /// Routes the URLs the app is opened with to `onForgeDeepLink` handlers.
    /// Attach once, to the root view.
    @MainActor
    public func receivesDeepLinks() -> some View {
        // Not `forgeOpenURLs()`: every hosted app's bindings declare it too
        // (friction log F19). Links for a hosted app go to that app's
        // ForgeDeepLinks; the rest are Device Lab's.
        onOpenURL { url in
            if !EmbeddedApps.openDeepLink(url) {
                ForgeDeepLinks.shared.open(url)
            }
        }
    }
}
