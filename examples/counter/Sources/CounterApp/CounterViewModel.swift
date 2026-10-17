import Foundation
import ForgeFFI

@MainActor
public final class CounterViewModel: ObservableObject {
    @Published public private(set) var count: Int64 = 0
    @Published public private(set) var startupLabel: String = ""
    @Published public var errorMessage: String?
    @Published public private(set) var isBlocked = false
    @Published public private(set) var canRecoverFromCorruption = false

    private var app: ForgeCounterApp?
    private var storageDirectory: String = ""

    public init() {
        storageDirectory = CounterViewModel.applicationSupportDirectory()
        openApp()
    }

    public func retryOpen() {
        openApp()
    }

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
            let opened = try ForgeCounterApp(storageDirectory: storageDirectory)
            attach(app: opened, afterRecovery: false)
        } catch let error as ForgeAppError {
            presentBlocked(error: error)
        } catch {
            isBlocked = true
            errorMessage = error.localizedDescription
        }
    }

    private func attach(app: ForgeCounterApp, afterRecovery: Bool) {
        self.app = app
        isBlocked = false
        count = (try? app.snapshot())?.count ?? 0
        if afterRecovery {
            startupLabel = "Recovered — fresh counter"
        } else if let kind = try? app.startupKind() {
            switch kind {
            case .freshInstall:
                startupLabel = "Fresh install"
            case .restored:
                startupLabel = "Restored saved state"
            }
        }
    }

    private func apply(_ kind: ForgeActionKind) {
        guard let app else { return }
        errorMessage = nil
        do {
            let outcome = try app.apply(action: kind)
            count = outcome.snapshot.count
        } catch let error as ForgeAppError {
            errorMessage = CounterViewModel.message(for: error)
            if let snap = try? app.snapshot() {
                count = snap.count
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func presentBlocked(error: ForgeAppError) {
        isBlocked = true
        app = nil
        switch error {
        case .corruptPersistence:
            canRecoverFromCorruption = true
            errorMessage = "Stored counter data is invalid. You can reset storage or retry opening."
        case .persistenceReadFailed:
            errorMessage = "Could not read saved data. Check storage access and retry."
        case .storageDirectoryUnavailable:
            errorMessage = "Storage folder is unavailable. Retry after fixing permissions."
        default:
            errorMessage = CounterViewModel.message(for: error)
        }
    }

    private static func message(for error: ForgeAppError) -> String {
        switch error {
        case .wouldUnderflow: return "Count cannot go below zero."
        case .wouldOverflow: return "Count cannot exceed one million."
        case .persistenceFailed: return "Could not save counter state."
        case .corruptPersistence: return "Stored counter data is invalid."
        case .persistenceReadFailed: return "Could not read saved data."
        case .storageDirectoryUnavailable: return "Storage folder is unavailable."
        case .internalFailure: return "Internal error in Rust core."
        }
    }

    private static func applicationSupportDirectory() -> String {
        let urls = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let base = urls[0].appendingPathComponent("Counter", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
