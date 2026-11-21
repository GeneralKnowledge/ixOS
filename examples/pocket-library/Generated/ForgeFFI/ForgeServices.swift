// Forge native services: the iOS side of `forge_services::native_services!()`.
//
// Source of truth: crates/forge-services/swift/ in the Forge checkout.
// `forge build` / scripts/generate-bindings.sh copy these files into the
// app's Generated/ForgeFFI/ next to the generated bindings, so they compile
// in the ForgeFFI module and implement its protocols directly. Do not edit
// the copies; they are replaced on every build.

import Foundation

#if canImport(os)
import os

/// The adapters' own warnings go to the system log, so they show in
/// `forge device logs` and crash reports. (Apps log through `ForgeLog`,
/// which not every app exports, so the adapters can't rely on it.)
private let forgeServicesLog = Logger(subsystem: "org.forgemobile.forge", category: "services")
#endif

/// Logs a warning from a native service adapter. Never pass user content.
func forgeServicesWarn(_ message: String) {
    #if canImport(os)
    forgeServicesLog.warning("\(message, privacy: .public)")
    #endif
}

/// Logs a notice from a native service adapter. Never pass user content.
func forgeServicesNote(_ message: String) {
    #if canImport(os)
    forgeServicesLog.notice("\(message, privacy: .public)")
    #endif
}

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
            forgeServicesWarn(
                "native services not installed: the Rust crate has services interface v\(rustVersion), "
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
            forgeServicesWarn("native services not installed: \(error)")
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
                forgeServicesWarn("native service \(service.name) not installed: \(error)")
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

#if canImport(ObjectiveC)
    import ObjectiveC

    /// One table per process, shared by every copy of these adapters. An app
    /// that hosts other apps (Device Lab) links several Rust libraries, each
    /// with its own copy of this file in its own module, but iOS has one
    /// notification centre and one background task scheduler. The copies
    /// meet in a dictionary attached to that system object, under a fixed
    /// key, and lock on the object itself, so the lock and the table are the
    /// same for all of them.
    enum ForgeProcessTable {
        /// Never dereferenced: only its value is compared ("ForgeTab").
        private static let key = UnsafeRawPointer(bitPattern: 0x466F_7267_6554_6162)!

        /// Runs `body` with the table attached to `anchor`, under a lock on
        /// `anchor`.
        static func with<T>(_ anchor: AnyObject, _ body: (NSMutableDictionary) -> T) -> T {
            objc_sync_enter(anchor)
            defer { objc_sync_exit(anchor) }
            let table: NSMutableDictionary
            if let existing = objc_getAssociatedObject(anchor, key) as? NSMutableDictionary {
                table = existing
            } else {
                table = NSMutableDictionary()
                objc_setAssociatedObject(anchor, key, table, .OBJC_ASSOCIATION_RETAIN)
            }
            return body(table)
        }

        /// The dictionary under `name` in `table`, created when missing.
        static func section(_ table: NSMutableDictionary, _ name: String) -> NSMutableDictionary {
            if let section = table[name] as? NSMutableDictionary { return section }
            let section = NSMutableDictionary()
            table[name] = section
            return section
        }
    }
#endif
