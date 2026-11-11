import ChecklistFFI
import ForgeKit
import Foundation
import Observation

extension ChecklistCore: ForgeCore {}

extension ChecklistError: ForgeKindedError {
    public var forgeKind: ErrorKind { checklistErrorKind(error: self) }
}

public enum Checklist {
    /// The app's host: one per process, made in the App.
    @MainActor
    public static func makeHost() -> ForgeHost<ChecklistCore> {
        ForgeHost(app: "Checklist", open: openCore)
    }

    /// A host on throwaway data, for tests.
    @MainActor
    public static func makeTemporaryHost() -> ForgeHost<ChecklistCore> {
        ForgeHost(environment: { try ForgeEnvironment.temporary() }, open: openCore)
    }

    @Sendable
    static func openCore(_ environment: ForgeEnvironment) throws -> ChecklistCore {
        try ChecklistCore(
            dataDirectory: environment.dataDirectory,
            cachesDirectory: environment.cachesDirectory
        )
    }

    /// What to tell the person about an error.
    public static func message(for error: Error) -> String {
        switch error as? ChecklistError {
        case .EmptyTitle?: return "Type something to add."
        case .TitleTooLong(let max)?: return "Keep it under \(max) characters."
        case .InUse?: return "The checklist is open somewhere else. Close the other copy and try again."
        default: return ForgeErrors.describe(error)
        }
    }
}

/// The checklist screen's state. Rust owns the items; this keeps the last
/// snapshot and re-reads it whenever the host says something changed.
@Observable
@MainActor
public final class ChecklistModel {
    public private(set) var items: [Item] = []
    public private(set) var summary = Summary(total: 0, done: 0)
    /// Set when a command fails; the view shows it and clears it.
    public var errorMessage: String?

    @ObservationIgnored private let core: ChecklistCore

    public init(core: ChecklistCore) {
        self.core = core
        reload()
    }

    /// Re-reads the snapshot. Cheap: Rust hands over its published copy
    /// without waiting for any command.
    public func reload() {
        items = core.items()
        summary = core.summary()
    }

    /// Adds an item; false (with `errorMessage` set) if it was refused.
    @discardableResult
    public func add(_ title: String) async -> Bool {
        await perform { _ = try await self.core.add(title: title) }
    }

    public func toggle(_ item: Item) async {
        await perform { try await self.core.toggle(id: item.id) }
    }

    @discardableResult
    public func rename(_ item: Item, to title: String) async -> Bool {
        await perform { try await self.core.rename(id: item.id, title: title) }
    }

    public func remove(_ item: Item) async {
        await perform { try await self.core.remove(id: item.id) }
    }

    public func clearDone() async {
        await perform { _ = try await self.core.clearDone() }
    }

    /// The items matching `query`, or nil if the search was stopped (the
    /// task was cancelled, or the app went to the background). Cancelling
    /// the calling task stops the search in Rust.
    public func search(_ query: String) async -> [Item]? {
        do {
            return try await withRustCancellation { cancellation in
                try await self.core.search(query: query, cancellation: cancellation)
            }
        } catch {
            return nil
        }
    }

    /// Writes the checklist as text for sharing; nil (with `errorMessage`
    /// set) if it couldn't.
    public func exportText() async -> URL? {
        do {
            let path = try await withRustCancellation { cancellation in
                try await self.core.exportText(cancellation: cancellation)
            }
            return URL(fileURLWithPath: path)
        } catch {
            errorMessage = Checklist.message(for: error)
            return nil
        }
    }

    /// Runs a command. The host's `changes` moves once it commits, and the
    /// view calls `reload()`; reloading here as well shows the result
    /// without waiting for the event.
    @discardableResult
    private func perform(_ command: () async throws -> Void) async -> Bool {
        do {
            try await command()
            reload()
            return true
        } catch {
            errorMessage = Checklist.message(for: error)
            return false
        }
    }
}
