// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it, and tells the screen when Rust has
// committed a change. Entry points import BusinessPreviewApp only; ForgeKit comes with
// it.
@_exported import ForgeKit
@preconcurrency import BusinessPreviewFFI
import Foundation

extension BusinessPreviewCore: ForgeCore {}

extension BusinessPreviewError: ForgeKindedError {
    /// The framework kind of each error (`business_preview_error_kind` in Rust).
    public var forgeKind: ErrorKind { businessPreviewErrorKind(error: self) }
}

/// The host for Business Preview's core, with its data in Application Support (or
/// throwaway data when launched with `-ForgeTemporaryData`, as UI tests
/// are). Make it once, in the `App`, never in a view.
@MainActor
public func makeAppHost() -> ForgeHost<BusinessPreviewCore> {
    ForgeHost(app: "BusinessPreview") { environment in
        try BusinessPreviewCore(storageDirectory: environment.dataDirectory)
    }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<BusinessPreviewCore> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) { environment in
        try BusinessPreviewCore(storageDirectory: environment.dataDirectory)
    }
}
