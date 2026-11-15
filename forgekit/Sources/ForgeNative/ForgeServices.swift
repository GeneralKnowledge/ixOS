// Forge native services: the iOS side of the forge-native crate, in
// ForgeKit's ForgeNative module next to that crate's generated bindings
// (forge_native.swift), so the adapters implement its protocols directly.
// Apps import ForgeNative; nothing is copied into an app's Generated/.
// Apple platforms only: on Linux this module holds just the bindings, which
// is what a model's tests need.

#if canImport(Darwin)

import Foundation

/// The services-interface version these adapters implement. Must equal
/// `nativeServicesVersion()` from the Rust side (forge_services::VERSION).
/// Both now ship together (the forge-native crate at the ixOS commit of this
/// ForgeKit release), so it only catches an app pinning a different commit.
let forgeServicesSwiftVersion: UInt32 = 3

/// Installs Forge's iOS implementations of the native services, so the app's
/// Rust code can use them (`forge_native::NativeServices::installed()`).
///
/// Call it once, as early as possible: from your `App`'s `init()`. That also
/// makes the notification delegate ready before a notification tap launches
/// the app.
public enum ForgeServices {
    /// - Parameter keychainService: the Keychain service name secrets are
    ///   stored under. Defaults to `<bundle id>.forge-secure-store`.
    /// - Parameter optional: installers of optional services, from their own
    ///   ForgeKit products. One that fails is logged and left out; Rust then
    ///   gets `NotInstalled` for it alone.
    /// - Returns: false if they could not be installed (the reason is logged);
    ///   Rust then gets `NotInstalled` errors.
    @discardableResult
    public static func install(
        keychainService: String? = nil,
        optional: [(name: String, install: @Sendable () throws -> Void)] = []
    ) -> Bool {
        let rustVersion = nativeServicesVersion()
        guard rustVersion == forgeServicesSwiftVersion else {
            print(
                "[forge] native services not installed: the Rust crate has services interface v\(rustVersion), "
                    + "the Swift adapters v\(forgeServicesSwiftVersion). Rebuild with the same Forge checkout "
                    + "(pin forge-native to the ixOS commit of this ForgeKit release).")
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
        for service in optional {
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

#endif
