@preconcurrency import ForgeApp
import Foundation
import Observation

/// The version of the forge-app contract this ForgeKit was built against.
/// `ForgeHost.start()` refuses a Rust library that implements another one.
public let forgeKitContractVersion: UInt32 = 1

/// What ForgeKit needs from an app's Rust core. An app's generated core
/// object already has these methods (the runtime contract it delegates to
/// `forge_app::Runtime`), so conforming is one line:
///
///     extension AppCore: ForgeCore {}
public protocol ForgeCore: AnyObject {
    func setPhase(phase: AppPhase)
    func events() -> RuntimeEvents?
    func shutdown()
}

/// Directories the runtime gives the core. The app decides what goes in
/// them; ForgeKit decides where they are and their backup and protection
/// policy (phase 4 adds the policies).
public struct ForgeEnvironment: Sendable, Equatable {
    public let dataDirectory: String
    public let cachesDirectory: String

    public init(dataDirectory: String, cachesDirectory: String) {
        self.dataDirectory = dataDirectory
        self.cachesDirectory = cachesDirectory
    }

    /// The launch argument that makes `standard(app:)` use throwaway data:
    /// UI tests and `forge ui explore` pass it so they never touch the
    /// person's own data.
    public static let temporaryDataArgument = "-ForgeTemporaryData"

    /// Application Support/<app>/data and Caches/<app>, or a temporary
    /// environment when the app was launched with `temporaryDataArgument`.
    public static func standard(app: String) throws -> ForgeEnvironment {
        if ProcessInfo.processInfo.arguments.contains(temporaryDataArgument) {
            return try temporary()
        }
        let fm = FileManager.default
        let support = try fm.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let caches = try fm.url(for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let data = support.appendingPathComponent(app).appendingPathComponent("data", isDirectory: true)
        let cache = caches.appendingPathComponent(app, isDirectory: true)
        for dir in [data, cache] { try fm.createDirectory(at: dir, withIntermediateDirectories: true) }
        return ForgeEnvironment(dataDirectory: data.path, cachesDirectory: cache.path)
    }

    /// A fresh throwaway environment, for tests and UI-test data.
    public static func temporary() throws -> ForgeEnvironment {
        let base = FileManager.default.temporaryDirectory
            .appendingPathComponent("forgekit-\(UUID().uuidString)", isDirectory: true)
        let data = base.appendingPathComponent("data", isDirectory: true)
        let cache = base.appendingPathComponent("caches", isDirectory: true)
        for dir in [data, cache] { try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true) }
        return ForgeEnvironment(dataDirectory: data.path, cachesDirectory: cache.path)
    }
}

/// Owns an app's Rust core for the life of the process. Create it once, in
/// the `App` (never in a view), and hand `core` to models.
@Observable
@MainActor
public final class ForgeHost<Core: ForgeCore> {
    public enum Status: Equatable {
        case loading
        case ready
        case failed(String)
    }

    public private(set) var status: Status = .loading
    public private(set) var core: Core?
    /// Increases whenever Rust's state may have changed (a commit, missed
    /// events, a return from the background). Models re-query when it moves.
    public private(set) var changes: UInt64 = 0
    /// The last committed revision Rust reported.
    public private(set) var revision: UInt64 = 0
    /// The application lifecycle phase last reported.
    public private(set) var phase: AppPhase = .launching

    @ObservationIgnored private let environment: @Sendable () throws -> ForgeEnvironment
    @ObservationIgnored private let opener: @Sendable (ForgeEnvironment) throws -> Core
    @ObservationIgnored private var listener: Task<Void, Never>?
    @ObservationIgnored private var stream: RuntimeEvents?
    @ObservationIgnored private var opening = false

    /// A host for the app named `app`, with its data in the standard
    /// places (`ForgeEnvironment.standard(app:)`). `open` makes the core;
    /// it runs off the main actor, once.
    public convenience init(app: String, open: @escaping @Sendable (ForgeEnvironment) throws -> Core) {
        self.init(environment: { try ForgeEnvironment.standard(app: app) }, open: open)
    }

    /// A host with its directories decided by `environment` (tests pass
    /// `ForgeEnvironment.temporary`).
    public init(
        environment: @escaping @Sendable () throws -> ForgeEnvironment,
        open: @escaping @Sendable (ForgeEnvironment) throws -> Core
    ) {
        self.environment = environment
        self.opener = open
    }

    /// Opens the core off the main actor and starts the event loop. Does
    /// nothing once open, so it is safe to call from any `.task`.
    public func start() async {
        // One core per process: a second start while the first is opening
        // would find the data directory held and fail.
        guard core == nil, !opening else { return }
        opening = true
        defer { opening = false }
        status = .loading
        let linked = forgeAppContractVersion()
        guard linked == forgeKitContractVersion else {
            status = .failed("This build mixes framework versions (Rust contract \(linked), ForgeKit \(forgeKitContractVersion)).")
            return
        }
        let environment = self.environment
        let opener = self.opener
        do {
            let core = try await Task.detached { try opener(try environment()) }.value
            // Subscribe before anyone reads, so no change is missed.
            let stream = core.events()
            self.core = core
            self.stream = stream
            if let stream { listen(to: stream) }
            if phase != .launching { core.setPhase(phase: phase) }
            status = .ready
            changes &+= 1
        } catch {
            status = .failed(ForgeErrors.describe(error))
        }
    }

    /// Reports the application lifecycle. `ForgeLifecycle` calls this from
    /// UIApplication notifications; tests call it directly.
    public func report(_ phase: AppPhase) {
        self.phase = phase
        if phase == .terminated {
            close()
        } else {
            core?.setPhase(phase: phase)
        }
    }

    /// Ends the event loop and shuts the core down. Committed work is
    /// already on disk.
    public func close() {
        stream?.cancel()
        listener?.cancel()
        core?.shutdown()
        stream = nil
        core = nil
    }

    private func listen(to stream: RuntimeEvents) {
        listener = Task { [weak self] in
            while let event = await stream.next() {
                guard let self else { return }
                switch event {
                case .changed(let revision):
                    self.revision = revision
                    self.changes &+= 1
                case .eventsDropped:
                    self.changes &+= 1
                case .phaseChanged(_, let resumed):
                    if resumed { self.changes &+= 1 }
                }
            }
        }
    }
}

/// Runs a cancellable Rust call so that cancelling the Swift task (a view
/// closing, `.task(id:)` restarting) cancels the work in Rust too. UniFFI
/// does not forward Task cancellation by itself.
public func withRustCancellation<T>(_ body: (Cancellation) async throws -> T) async throws -> T {
    let cancellation = Cancellation()
    return try await withTaskCancellationHandler {
        try await body(cancellation)
    } onCancel: {
        cancellation.cancel()
    }
}

/// An app error that knows its framework kind. Apps conform their
/// generated error with one line: `var forgeKind: ErrorKind { errorKind(error: self) }`.
public protocol ForgeKindedError: Error {
    var forgeKind: ErrorKind { get }
}

public enum ForgeErrors {
    /// Default text for an error, by kind. Apps override for their domain cases.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? ForgeKindedError else { return error.localizedDescription }
        return describe(error.forgeKind)
    }

    public static func describe(_ kind: ErrorKind) -> String {
        switch kind {
        case .invalid: return "Check what you entered."
        case .notFound: return "That no longer exists."
        case .unavailable: return "Not available right now. Try again later."
        case .interrupted: return "Stopped when the app went to the background. Try again."
        case .cancelled: return "Cancelled."
        case .needsUser: return "This needs your permission or sign-in."
        case .corrupt: return "The saved data couldn't be read."
        case .newerData: return "This data was saved by a newer version of the app. Update the app to open it."
        case .shutDown: return "The app is closing."
        case .internal: return "Something went wrong."
        }
    }
}
