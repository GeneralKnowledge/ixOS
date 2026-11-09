@preconcurrency import ForgeFFI
import Foundation
import Observation

/// What the screen shows and the actions it can take. It holds no rules of
/// its own: every change goes to the Rust `AppCore` (your code in
/// `rust/src/lib.rs`), and the list shown is the list Rust returns.
///
/// It imports no SwiftUI, so `forge check swift` compiles and tests it on
/// Linux. Views read its properties and call its `async` methods.
@Observable
@MainActor
public final class ItemsModel {
    /// How far opening the Rust core has got.
    public enum Phase: Equatable {
        case loading
        case ready
        case failed(String)
    }

    public private(set) var phase: Phase = .loading
    public private(set) var items: [Item] = []
    /// A failed action, for an alert. The view clears it when dismissed.
    public var errorMessage: String?

    private var core: AppCore?
    private let storageDirectory: String?

    /// - Parameter storageDirectory: where Rust keeps `items.json`. Defaults
    ///   to the app's Application Support folder; tests pass a temporary one.
    public init(storageDirectory: String? = nil) {
        self.storageDirectory = storageDirectory
    }

    /// Opens the Rust core. Calling it again once open does nothing.
    public func open() async {
        guard core == nil else { return }
        phase = .loading
        do {
            let directory = try storageDirectory ?? Self.applicationSupportDirectory()
            // Opening reads a file: do it off the main actor.
            let core = try await Task.detached { try AppCore(storageDirectory: directory) }.value
            self.core = core
            items = core.items()
            phase = .ready
        } catch {
            phase = .failed(Self.describe(error))
        }
    }

    public func add(title: String) async {
        await perform { core in try await core.addItem(title: title) }
    }

    public func toggle(_ item: Item) async {
        await perform { core in try await core.toggleItem(id: item.id) }
    }

    public func delete(_ item: Item) async {
        await perform { core in try await core.deleteItem(id: item.id) }
    }

    /// Runs one Rust command and shows the list it returns. A failure goes
    /// to `errorMessage`.
    private func perform(_ command: (AppCore) async throws -> [Item]) async {
        guard let core else { return }
        do {
            items = try await command(core)
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    /// Text for the screen. `reason` fields are diagnostics from Rust.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return error.localizedDescription }
        switch error {
        case .InvalidInput(let reason): return "Check your input: \(reason)"
        case .NotFound: return "That item no longer exists."
        case .Storage(let reason): return "Your list couldn't be read or saved (\(reason))."
        }
    }

    private static func applicationSupportDirectory() throws -> String {
        let base = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("FORGE_APP_SUPPORT_DIR", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
