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
        case .InvalidDay?: return "That isn't a day the checklist can use. Pick another."
        default: return ForgeErrors.describe(error)
        }
    }
}

/// Due days as Rust keeps them: `YYYY-MM-DD` on the Gregorian calendar, in
/// the phone's time zone. Always Gregorian, whatever calendar the person
/// uses, so a Buddhist-calendar phone doesn't save the year 2569.
public enum ChecklistDay {
    static func calendar() -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        return calendar
    }

    /// The local day `date` falls on.
    public static func string(from date: Date) -> String {
        let parts = calendar().dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
    }

    /// The start of a saved day, or nil if it isn't one.
    public static func date(from day: String) -> Date? {
        let parts = day.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        return calendar().date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
    }
}

/// The checklist screen's state. Rust owns the items; this keeps the last
/// snapshot and re-reads it whenever the host says something changed.
@Observable
@MainActor
public final class ChecklistModel {
    public private(set) var items: [Item] = []
    public private(set) var summary = Summary(total: 0, done: 0)
    /// What is overdue, due today and coming up, on today's local day.
    public private(set) var agenda = Agenda(overdue: [], today: [], upcoming: [])
    /// Today, as Rust compares due days. Read again with every reload, so
    /// the agenda moves on after midnight once anything changes or the app
    /// comes back.
    public private(set) var today = ChecklistDay.string(from: Date())
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
        today = ChecklistDay.string(from: Date())
        agenda = core.agenda(today: today)
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

    /// Sets the day an item is due, or clears it with nil. Throws, so the
    /// Due Date sheet can show a refusal itself.
    public func setDue(_ item: Item, to date: Date?) async throws {
        try await core.setDue(id: item.id, due: date.map(ChecklistDay.string(from:)))
        reload()
    }

    public func remove(_ item: Item) async {
        await perform { try await self.core.remove(id: item.id) }
    }

    public func clearDone() async {
        await perform { _ = try await self.core.clearDone() }
    }

    /// The items matching `query`, or nil if the search was stopped (the
    /// task was cancelled, or the app went to the background) or failed.
    /// Cancelling the calling task stops the search in Rust. A failure that
    /// isn't a stop sets `errorMessage`, like the other commands, so a
    /// broken search doesn't look like an empty one.
    public func search(_ query: String) async -> [Item]? {
        do {
            return try await withRustCancellation { cancellation in
                try await self.core.search(query: query, cancellation: cancellation)
            }
        } catch {
            if !Self.isStop(error) { errorMessage = Checklist.message(for: error) }
            return nil
        }
    }

    /// A cancelled or interrupted command: not something to report.
    nonisolated public static func isStop(_ error: Error) -> Bool {
        if error is CancellationError { return true }
        if case .Stopped? = error as? ChecklistError { return true }
        return false
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

    /// Writes the checklist as a spreadsheet (Item, Done) for sharing; nil
    /// (with `errorMessage` set) if it couldn't.
    public func exportCSV() async -> URL? {
        do {
            let path = try await withRustCancellation { cancellation in
                try await self.core.exportCsv(cancellation: cancellation)
            }
            return URL(fileURLWithPath: path)
        } catch {
            errorMessage = Checklist.message(for: error)
            return nil
        }
    }

    /// Adds the rows of a spreadsheet chosen in the document picker. The
    /// picker's URL is security-scoped: access is held while Rust reads it.
    public func importCSV(at url: URL) async -> CsvImportSummary? {
        #if canImport(Darwin)
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        #endif
        do {
            let summary = try await core.importCsv(path: url.path)
            reload()
            return summary
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
