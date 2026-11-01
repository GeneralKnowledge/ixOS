import Foundation
import ForgeFFI

@MainActor
public final class CounterViewModel: ObservableObject {
    @Published public private(set) var count: Int64 = 0
    @Published public private(set) var startupLabel: String = ""
    @Published public var errorMessage: String?
    @Published public private(set) var isBlocked = false
    /// True only when stored data is corrupt. Data saved by a newer app
    /// version is valid, so it is never offered for reset.
    @Published public private(set) var canRecoverFromCorruption = false

    private var app: ForgeCounterApp?
    private let storageDirectory: String

    /// - Parameter storageDirectory: where Rust keeps its snapshot. Defaults to
    ///   the app's Application Support folder; tests pass a temporary folder.
    public init(storageDirectory: String? = nil) {
        self.storageDirectory = storageDirectory ?? Self.applicationSupportDirectory()
        openApp()
    }

    public func retryOpen() { openApp() }

    public func recoverFromCorruption() {
        errorMessage = nil
        do {
            let recovered = try recoverCounterApp(storageDirectory: storageDirectory)
            attach(app: recovered, afterRecovery: true)
        } catch let error as ForgeAppError {
            presentBlocked(error: error)
        } catch {
            isBlocked = true
            errorMessage = error.localizedDescription
        }
    }

    public func increment() { apply(.increment) }
    public func decrement() { apply(.decrement) }
    public func reset() { apply(.reset) }

    private func openApp() {
        errorMessage = nil
        isBlocked = false
        canRecoverFromCorruption = false
        do {
            attach(app: try ForgeCounterApp(storageDirectory: storageDirectory), afterRecovery: false)
        } catch let error as ForgeAppError {
            presentBlocked(error: error)
        } catch {
            isBlocked = true
            errorMessage = error.localizedDescription
        }
    }

    private func attach(app: ForgeCounterApp, afterRecovery: Bool) {
        do {
            let snapshot = try app.snapshot()
            let kind = try app.startupKind()
            self.app = app
            isBlocked = false
            canRecoverFromCorruption = false
            count = snapshot.count
            if afterRecovery && kind == .freshInstall {
                startupLabel = "Recovered — fresh counter"
            } else {
                startupLabel = kind == .freshInstall ? "Fresh install" : "Restored saved state"
            }
        } catch let error as ForgeAppError {
            // Never show a made-up count when Rust could not report the real one.
            presentBlocked(error: error)
        } catch {
            isBlocked = true
            errorMessage = error.localizedDescription
        }
    }

    private func apply(_ kind: ForgeActionKind) {
        guard let app else { return }
        errorMessage = nil
        do {
            count = try app.apply(action: kind).snapshot.count
        } catch let error as ForgeAppError {
            errorMessage = Self.message(for: error)
            if let snap = try? app.snapshot() { count = snap.count }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func presentBlocked(error: ForgeAppError) {
        isBlocked = true
        app = nil
        canRecoverFromCorruption = false
        switch error {
        case .CorruptPersistence:
            canRecoverFromCorruption = true
            errorMessage = "Stored counter data is invalid. You can reset storage or retry opening."
        case .UnsupportedSchemaVersion:
            errorMessage = "Your counter was saved by a newer version of this app. Update the app to open it; your data has not been changed."
        case .PersistenceReadFailed:
            errorMessage = "Could not read saved data. Check storage access and retry."
        case .StorageDirectoryUnavailable:
            errorMessage = "Storage folder is unavailable. Retry after fixing permissions."
        default:
            errorMessage = Self.message(for: error)
        }
    }

    private static func message(for error: ForgeAppError) -> String {
        switch error {
        case .WouldUnderflow: return "Count cannot go below zero."
        case .WouldOverflow: return "Count cannot exceed one million."
        case .PersistenceFailed: return "Could not save counter state."
        case .CorruptPersistence: return "Stored counter data is invalid."
        case .UnsupportedSchemaVersion: return "Saved data is from a newer version of this app."
        case .PersistenceReadFailed: return "Could not read saved data."
        case .StorageDirectoryUnavailable: return "Storage folder is unavailable."
        case .InternalFailure: return "Internal error in Rust core."
        }
    }

    private static func applicationSupportDirectory() -> String {
        let urls = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let base = urls[0].appendingPathComponent("Counter", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
