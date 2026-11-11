@preconcurrency import FORGE_MODULE_NAMEFFI
import Foundation
import Observation

/// What the screen shows and the actions it can take. It holds no rules of
/// its own: every change goes to the Rust `AppCore` (your code in
/// `rust/src/lib.rs`), and the list shown is the list Rust returns.
///
/// ForgeKit's `ForgeHost` opens the core once per process (FORGE_MODULE_NAMEApp.swift)
/// and gives it to this model. When Rust commits a change, the host's
/// `changes` moves and the view calls `reload()`.
///
/// It imports no SwiftUI, so `forge check swift` compiles and tests it on
/// Linux. Views read its properties and call its `async` methods.
@Observable
@MainActor
public final class ItemsModel {
    public private(set) var items: [Item] = []
    /// A failed action, for an alert. The view clears it when dismissed.
    public var errorMessage: String?

    public let core: AppCore

    public init(core: AppCore) {
        self.core = core
        items = core.items()
    }

    /// Re-reads the list from Rust. Cheap: it never waits for a command.
    public func reload() {
        items = core.items()
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
        do {
            items = try await command(core)
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    /// Text for the screen. `reason` fields are diagnostics from Rust: only
    /// `InvalidInput`'s, which says what to fix, is shown.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return error.localizedDescription }
        switch error {
        case .InvalidInput(let reason): return "Check your input: \(reason)"
        case .NotFound: return "That item no longer exists."
        // `reason` is a diagnostic for the log, never the screen.
        case .Storage: return "Your list couldn't be read or saved. Try again, or close the app and open it again."
        case .InUse: return "Your list is open in another copy of the app."
        case .ShutDown: return "The app is closing."
        }
    }
}
