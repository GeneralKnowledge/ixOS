// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screens when Rust has
// committed a change. Entry points import SoundscapeApp only; ForgeKit comes
// with it.
@_exported import ForgeKit
@preconcurrency import SoundscapeFFI
import Foundation

extension SoundscapeCore: ForgeCore {}

extension SoundscapeError: ForgeKindedError {
    /// The framework kind of each error (`soundscape_error_kind` in Rust).
    public var forgeKind: ErrorKind { soundscapeErrorKind(error: self) }
}

/// The host for Soundscape's core, with its data in Application Support (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<SoundscapeCore> {
    ForgeHost(app: "Soundscape", open: openCore)
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<SoundscapeCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }, open: openCore)
}

private func openCore(_ environment: ForgeEnvironment) throws -> SoundscapeCore {
    try SoundscapeCore(
        dataDirectory: environment.dataDirectory,
        cachesDirectory: environment.cachesDirectory
    )
}
