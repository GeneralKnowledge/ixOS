@preconcurrency import ForgeApp
@preconcurrency import ForgeFFI
import ForgeKit
import Foundation
import Observation

// The two lines that connect this app to ForgeKit. In the final design
// `forge bindings generate` writes them.
extension AppCore: ForgeCore {}
extension AppError: ForgeKindedError {
    public var forgeKind: ErrorKind { errorKind(error: self) }
}

/// The checklist screen's model. It owns no core and no event loop: the
/// ForgeHost does, and this re-queries Rust when `host.changes` moves.
@Observable
@MainActor
public final class ItemsModel {
    public let host: ForgeHost<AppCore>
    public private(set) var items: [Item] = []
    public var errorMessage: String?
    public private(set) var countResult: String?

    public init(host: ForgeHost<AppCore>) {
        self.host = host
    }

    /// The host the App creates once.
    public static func makeHost(environment: ForgeEnvironment) -> ForgeHost<AppCore> {
        ForgeHost(environment: environment) { env in
            try AppCore(storageDirectory: env.dataDirectory)
        }
    }

    /// Re-reads what the screen shows. Views call it when `host.changes` moves.
    public func refresh() {
        items = host.core?.items() ?? []
    }

    public func add(title: String) async {
        await perform { core in _ = try await core.addItem(title: title) }
    }

    public func toggle(_ item: Item) async {
        await perform { core in try await core.toggleItem(id: item.id) }
    }

    /// Long on-screen work: cancelled when the calling task is cancelled,
    /// interrupted when the app goes to the background.
    public func count(to n: UInt32) async {
        guard let core = host.core else { return }
        countResult = nil
        do {
            let reached = try await withRustCancellation { c in
                try await core.slowCount(to: n, cancellation: c)
            }
            countResult = "Counted to \(reached)"
        } catch {
            countResult = ForgeErrors.describe(error)
        }
    }

    private func perform(_ command: (AppCore) async throws -> Void) async {
        guard let core = host.core else { return }
        do {
            try await command(core)
        } catch {
            errorMessage = ForgeErrors.describe(error)
        }
    }
}
