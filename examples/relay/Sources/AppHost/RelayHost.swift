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

/// The host for Relay's core, with its data in Application Support and
/// files received with a code in Documents/Relay (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<RelayCore> {
    let name = deviceName()
    // Files received with a code go to Documents/Relay, which the Files app
    // shows; throwaway data keeps them in its own folder.
    let documents = ForgeEnvironment.usesTemporaryData()
        ? nil
        : FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    return ForgeHost(app: "Relay") {
        try openCore($0, name: name, receivedFiles: documents?.appendingPathComponent("Relay", isDirectory: true))
    }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<RelayCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) {
        // Offline: a test never reaches Relay's real servers.
        try RelayCore.withoutDefaultServers(dataDirectory: $0.dataDirectory, deviceName: "Test Device")
    }
}

private func openCore(_ environment: ForgeEnvironment, name: String, receivedFiles: URL? = nil) throws -> RelayCore {
    // The first open names this device; the person can rename it.
    let core = try RelayCore(dataDirectory: environment.dataDirectory, deviceName: name)
    if let receivedFiles {
        try core.receiveInto(folder: receivedFiles.path)
    }
    return core
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
