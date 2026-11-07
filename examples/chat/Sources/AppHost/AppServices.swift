@preconcurrency import ForgeFFI
import SwiftUI

/// The native services (Keychain, app directories, local notifications,
/// permissions, deep links) that the Rust core uses through
/// `crate::native` (see `rust/src/lib.rs` and docs/native-services.md).
///
/// The app's entry point calls `AppServices.install()` once and attaches
/// `.receivesDeepLinks()` to the root view. Views that react to links use
/// `.onForgeDeepLink { link in ... }`; share buttons use `ForgeShareLink`.
public enum AppServices {
    /// Installs Forge's iOS implementations for the Rust core. Call it from
    /// `App.init()`, before any view opens the core.
    @MainActor
    public static func install() {
        ForgeServices.install()
        // The microphone for voice messages (M4).
        ForgeAudio.install()
    }
}

extension View {
    /// Routes the URLs the app is opened with (custom schemes from forge.toml,
    /// universal links) and notification taps to `onForgeDeepLink` handlers.
    /// Attach once, to the root view.
    @MainActor
    public func receivesDeepLinks() -> some View {
        forgeOpenURLs()
    }
}
