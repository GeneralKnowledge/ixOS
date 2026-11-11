// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screens when Rust has
// committed a change. Entry points import FieldKitApp only; ForgeKit comes
// with it.
@_exported import ForgeKit
@preconcurrency import FieldKitFFI
import Foundation

extension FieldKitCore: ForgeCore {}

extension FieldKitError: ForgeKindedError {
    /// The framework kind of each error (`fieldkit_error_kind` in Rust).
    public var forgeKind: ErrorKind { fieldkitErrorKind(error: self) }
}

/// The host for FieldKit's core, with its data in Application Support (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<FieldKitCore> {
    ForgeHost(app: "FieldKit", open: openCore)
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<FieldKitCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }, open: openCore)
}

private func openCore(_ environment: ForgeEnvironment) throws -> FieldKitCore {
    try FieldKitCore(
        dataDirectory: environment.dataDirectory,
        cachesDirectory: environment.cachesDirectory
    )
}
