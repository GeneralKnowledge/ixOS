import Foundation
import ForgeFFI

@MainActor
final class CounterViewModel: ObservableObject {
    @Published private(set) var count: Int64 = 0
    @Published private(set) var persistenceOk = true
    @Published var errorMessage: String?

    private let app: ForgeCounterApp

    init() {
        let dir = CounterViewModel.applicationSupportDirectory()
        do {
            app = try ForgeCounterApp(storageDirectory: dir)
            count = app.snapshot().count
            persistenceOk = app.lastPersistenceOk()
        } catch {
            fatalError("Failed to initialize Rust counter: \(error)")
        }
    }

    func increment() { apply(.increment) }
    func decrement() { apply(.decrement) }
    func reset() { apply(.reset) }

    private func apply(_ kind: ForgeActionKind) {
        errorMessage = nil
        do {
            let outcome = try app.apply(action: kind)
            count = outcome.snapshot.count
            persistenceOk = outcome.persistenceOk && app.lastPersistenceOk()
        } catch let error as ForgeAppError {
            errorMessage = CounterViewModel.message(for: error)
            count = app.snapshot().count
            persistenceOk = app.lastPersistenceOk()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private static func message(for error: ForgeAppError) -> String {
        switch error {
        case .wouldUnderflow: return "Count cannot go below zero."
        case .wouldOverflow: return "Count cannot exceed one million."
        case .persistenceFailed: return "Could not save counter state."
        case .corruptPersistence: return "Stored data was invalid; using safe defaults."
        }
    }

    private static func applicationSupportDirectory() -> String {
        let urls = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let base = urls[0].appendingPathComponent("Counter", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
