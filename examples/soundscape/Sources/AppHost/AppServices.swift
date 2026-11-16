@preconcurrency import SoundscapeFFI

/// Forge's native services for the Rust core (see `rust/src/lib.rs` and
/// docs/native-services.md): the core set plus `http`, so Listen's requests
/// to the take server go through URLSession and follow the phone's VPN
/// (Tailscale).
///
/// The app's entry point calls `AppServices.install()` once, from
/// `App.init()`. Device Lab installs Soundscape's services itself.
public enum AppServices {
    @MainActor
    public static func install() {
        ForgeServices.install()
    }
}
