// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screens when Rust has
// committed a change. Entry points import RelayApp only; ForgeKit comes
// with it.
@_exported import ForgeKit
@preconcurrency import RelayFFI
import Foundation
#if canImport(UIKit)
import UIKit
#endif

extension RelayCore: ForgeCore {}

extension RelayError: ForgeKindedError {
    /// The framework kind of each error (`relay_error_kind` in Rust).
    public var forgeKind: ErrorKind { relayErrorKind(error: self) }
}

/// The host for Relay's core, with its data in Application Support (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<RelayCore> {
    let name = deviceName()
    return ForgeHost(app: "Relay") { try openCore($0, name: name) }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<RelayCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) { try openCore($0, name: "Test Device") }
}

private func openCore(_ environment: ForgeEnvironment, name: String) throws -> RelayCore {
    // The first open names this device; the person can rename it.
    try RelayCore(dataDirectory: environment.dataDirectory, deviceName: name)
}

@MainActor
private func deviceName() -> String {
    #if canImport(UIKit)
    // "iPhone" without the user-assigned-name entitlement, which is fine.
    return UIDevice.current.name
    #else
    return ProcessInfo.processInfo.hostName
    #endif
}
