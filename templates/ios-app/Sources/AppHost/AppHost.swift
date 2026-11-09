// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screens when Rust has
// committed a change. Entry points import AppHost only; ForgeKit comes with
// it.
@_exported import ForgeKit
@preconcurrency import ForgeFFI
import Foundation

extension AppCore: ForgeCore {}

extension AppError: ForgeKindedError {
    /// The framework kind of each error (`app_error_kind` in Rust).
    public var forgeKind: ErrorKind { appErrorKind(error: self) }
}

/// The host for this app's core, with its data in Application Support
/// (or throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<AppCore> {
    ForgeHost(app: "FORGE_APP_SUPPORT_DIR") { environment in
        try AppCore(storageDirectory: environment.dataDirectory)
    }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<AppCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) { environment in
        try AppCore(storageDirectory: environment.dataDirectory)
    }
}
