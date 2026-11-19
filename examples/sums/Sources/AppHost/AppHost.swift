// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screen when Rust has
// committed a change. Entry points import SumsApp only; ForgeKit comes with
// it.
@_exported import ForgeKit
@preconcurrency import SumsFFI
import Foundation

extension SumsCore: ForgeCore {}

extension SumsError: ForgeKindedError {
    /// The framework kind of each error (`sums_error_kind` in Rust).
    public var forgeKind: ErrorKind { sumsErrorKind(error: self) }
}

/// The host for Sums' core, with its data in Application Support (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<SumsCore> {
    ForgeHost(app: "Sums") { environment in
        try SumsCore(storageDirectory: environment.dataDirectory)
    }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<SumsCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) { environment in
        try SumsCore(storageDirectory: environment.dataDirectory)
    }
}
