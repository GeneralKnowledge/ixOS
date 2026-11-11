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

    /// Routes a URL the app was opened with: links for a hosted app go to
    /// that app's ForgeDeepLinks, the rest to `onForgeDeepLink` handlers.
    /// ContentView calls it from `onOpenURL`. Not a View helper called from
    /// the App, as in the other apps: three hosted apps declare a public
    /// `receivesDeepLinks()` too, and `forgeOpenURLs()` comes with every
    /// app's bindings, so either call is ambiguous here (friction log F19).
    @MainActor
    public static func open(_ url: URL) {
        if !EmbeddedApps.openDeepLink(url) {
            ForgeDeepLinks.shared.open(url)
        }
    }
}
