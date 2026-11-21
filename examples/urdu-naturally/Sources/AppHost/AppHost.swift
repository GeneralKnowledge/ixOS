// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screen when Rust has
// committed a change. Entry points import UrduNaturallyApp only; ForgeKit comes with
// it.
@_exported import ForgeKit
@preconcurrency import UrduNaturallyFFI
import Foundation

extension UrduCore: ForgeCore {}

extension UrduError: ForgeKindedError {
    /// The framework kind of each error (`urdu_error_kind` in Rust).
    public var forgeKind: ErrorKind { urduErrorKind(error: self) }
}

/// The host for Urdu, Naturally's core, with its data in Application Support (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<UrduCore> {
    ForgeHost(app: "UrduNaturally") { environment in
        try UrduCore(storageDirectory: environment.dataDirectory)
    }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<UrduCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) { environment in
        try UrduCore(storageDirectory: environment.dataDirectory)
    }
}
