// Forge native services: the iOS side of `forge_services::native_services!()`.
//
// Source of truth: crates/forge-services/swift/ in the Forge checkout.
// `forge build` / scripts/generate-bindings.sh copy these files into the
// app's Generated/ForgeFFI/ next to the generated bindings, so they compile
// in the ForgeFFI module and implement its protocols directly. Do not edit
// the copies; they are replaced on every build.

import Foundation

/// The services-interface version these adapters implement. Must equal
/// `nativeServicesVersion()` from the Rust side (forge_services::VERSION).
let forgeServicesSwiftVersion: UInt32 = 3

/// Installs Forge's iOS implementations of the native services, so the app's
/// Rust code can use them (`native::NativeServices::installed()`).
///
/// Call it once, as early as possible: from your `App`'s `init()`. That also
/// makes the notification delegate ready before a notification tap launches
/// the app.
public enum ForgeServices {
    /// - Parameter keychainService: the Keychain service name secrets are
    ///   stored under. Defaults to `<bundle id>.forge-secure-store`.
    /// - Returns: false if they could not be installed (the reason is logged);
    ///   Rust then gets `NotInstalled` errors.
    @discardableResult
    public static func install(keychainService: String? = nil) -> Bool {
        let rustVersion = nativeServicesVersion()
        guard rustVersion == forgeServicesSwiftVersion else {
            print(
                "[forge] native services not installed: the Rust crate has services interface v\(rustVersion), "
                    + "the Swift adapters v\(forgeServicesSwiftVersion). Rebuild with the same Forge checkout "
                    + "(forge build regenerates both).")
            return false
        }
        do {
            try installNativeServices(
                directories: ForgeFiles.appDirectories(),
                secureStore: KeychainSecureStore(
                    service: keychainService ?? KeychainSecureStore.defaultService),
                notifier: ForgeNotifications.makeNotifier(),
                permissions: ForgePermissions.shared)
        } catch {
            print("[forge] native services not installed: \(error)")
            return false
        }
        // The optional services the Rust crate selected in
        // `native_services!(...)` (ForgeServices+Selected.swift, written by
        // generate-bindings.sh). One that fails is logged and left out; Rust
        // then gets `NotInstalled` for it alone.
        for service in forgeSelectedServices {
            do {
                try service.install()
            } catch {
                print("[forge] native service \(service.name) not installed: \(error)")
            }
        }
        return true
    }

    /// True when this process is an installed app bundle. Some services (the
    /// notification centre) do not exist in `swift test` or `swift run`.
    public static var isAppBundle: Bool {
        Bundle.main.bundleURL.pathExtension == "app" && Bundle.main.bundleIdentifier != nil
    }
}
