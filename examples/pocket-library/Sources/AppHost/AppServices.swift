@preconcurrency import PocketLibraryFFI
// The adapters need Apple's frameworks, so the Linux twin leaves this file
// out (as it does every file that imports SwiftUI).
import SwiftUI

/// The native services the Rust library uses through `crate::native` (see
/// `rust/src/lib.rs` and docs/native-services.md): above all `http`, so
/// catalogue requests go through URLSession on iOS. Before they are
/// installed, and in Linux tests, the library falls back to its own client.
public enum AppServices {
    /// Call once from `App.init()`, before the library opens. An app that
    /// hosts Pocket Library (Device Lab) calls
    /// `PocketLibraryFFI.ForgeServices.install(keychainService:)` itself.
    @MainActor
    public static func install() {
        ForgeServices.install()
    }
}
