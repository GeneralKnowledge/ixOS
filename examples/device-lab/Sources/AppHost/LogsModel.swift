@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The Logs screen's view of Forge's in-memory log (`rust/src/logs.rs`):
/// the last 512 records from Device Lab and every hosted app, newest first,
/// filtered by level and text. No UI imports, so it tests on Linux.
@MainActor
@Observable
public final class LogsModel {
    /// Which records the screen shows.
    public enum Filter: String, CaseIterable, Identifiable, Sendable {
        case all, warnings, errors
        public var id: String { rawValue }

        public var name: String {
            switch self {
            case .all: "All"
            case .warnings: "Warnings and Errors"
            case .errors: "Errors"
            }
        }

        func includes(_ level: LogLevel) -> Bool {
            switch self {
            case .all: true
            case .warnings: level == .warn || level == .error
            case .errors: level == .error
            }
        }
    }

    public var filter: Filter = .all
    public var search = ""
    /// Every kept record, newest first.
    public private(set) var all: [LogLine] = []

    @ObservationIgnored private let read: () -> [LogLine]

    public init(read: @escaping () -> [LogLine] = { recentLogs(limit: 512) }) {
        self.read = read
    }

    /// What the screen lists: the records that pass the filter and contain
    /// the search text (in the message or where it came from).
    public var shown: [LogLine] {
        let text = search.trimmingCharacters(in: .whitespaces)
        return all.filter { line in
            filter.includes(line.level)
                && (text.isEmpty
                    || line.message.localizedCaseInsensitiveContains(text)
                    || line.target.localizedCaseInsensitiveContains(text))
        }
    }

    /// The shown records as text, oldest first, for the share sheet.
    public var shareText: String {
        logsAsText(lines: Array(shown.reversed()))
    }

    public func refresh() {
        let next = Array(read().reversed())
        if next != all { all = next }
    }

    /// Re-reads every second until the calling task is cancelled (attach
    /// with `.task`), so new records appear while the screen is open.
    public func follow(every interval: Duration = .seconds(1)) async {
        while !Task.isCancelled {
            refresh()
            try? await Task.sleep(for: interval)
        }
    }
}
