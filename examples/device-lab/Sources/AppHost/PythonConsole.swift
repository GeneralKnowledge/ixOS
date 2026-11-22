import Foundation
import Observation

/// What a Python Lab run tells its console while it runs (PL1d).
public enum PythonConsoleEvent: Sendable, Equatable {
    /// The run has the interpreter now.
    case started
    /// It waits for a Bonsai Harness run to finish first.
    case queuedBehindBonsai
    /// Text the script wrote to stdout.
    case output(String)
    /// Text it wrote to stderr, tracebacks included.
    case error(String)
    /// `input()` is waiting for the person's answer.
    case waitingForInput
    /// `input()` stopped waiting: with the answer it got, or nil when Stop
    /// cancelled it.
    case inputEnded(String?)
}

/// Asks the editor to show a line (1 is the first), once per request.
public struct PythonLineRequest: Equatable, Sendable {
    public let id = UUID()
    public let line: Int

    public init(line: Int) {
        self.line = line
    }

    /// Where line `line` is in `text`, in UTF-16 units; the end of the
    /// text when it has fewer lines.
    public static func range(ofLine line: Int, in text: NSString) -> NSRange {
        var start = 0
        var current = 1
        while current < line, start < text.length {
            let rest = NSRange(location: start, length: text.length - start)
            let newline = text.range(of: "\n", options: [], range: rest)
            if newline.location == NSNotFound {
                return NSRange(location: text.length, length: 0)
            }
            start = newline.location + 1
            current += 1
        }
        let end = text.range(
            of: "\n", options: [], range: NSRange(location: start, length: text.length - start))
        let length = (end.location == NSNotFound ? text.length : end.location) - start
        return NSRange(location: start, length: length)
    }
}

/// A stretch of the console in one style.
public struct ConsoleSegment: Identifiable, Equatable, Sendable {
    public enum Kind: Sendable, Equatable {
        /// stdout, in the normal colour.
        case output
        /// stderr, in the danger tone.
        case error
        /// The person's answers to `input()`.
        case input
        /// Python Lab's own words (the time limit, no output).
        case note
    }

    public let id: Int
    public let kind: Kind
    public var text: String
}

/// The console's text: segments in order, consecutive ones of a kind
/// merged, keeping only the last `limit` bytes. What it removed is
/// counted, so the screen can say so rather than cut silently.
public struct PythonConsoleText: Equatable, Sendable {
    public private(set) var segments: [ConsoleSegment] = []
    /// Bytes removed from the start to stay under `limit`.
    public private(set) var removedBytes = 0
    public let limit: Int
    private var bytes = 0
    private var nextID = 0

    public init(limit: Int = 1_048_576) {
        self.limit = limit
    }

    public var isEmpty: Bool { segments.isEmpty && removedBytes == 0 }

    /// "Earlier output removed (N KB)", once anything was.
    public var removedNotice: String? {
        guard removedBytes > 0 else { return nil }
        return "Earlier output removed (\(max(1, (removedBytes + 512) / 1024)) KB)"
    }

    /// Everything as plain text, the notice first.
    public var plainText: String {
        let body = segments.map(\.text).joined()
        guard let removedNotice else { return body }
        return removedNotice + "\n" + body
    }

    public mutating func append(_ text: String, as kind: ConsoleSegment.Kind) {
        guard !text.isEmpty else { return }
        if let last = segments.indices.last, segments[last].kind == kind {
            segments[last].text += text
        } else {
            segments.append(ConsoleSegment(id: nextID, kind: kind, text: text))
            nextID += 1
        }
        bytes += text.utf8.count
        trim()
    }

    /// Drops the oldest text until the rest fits, cutting a segment at the
    /// start of a line where it can (else at a character).
    private mutating func trim() {
        while bytes > limit, !segments.isEmpty {
            let excess = bytes - limit
            let utf8 = Array(segments[0].text.utf8)
            if utf8.count <= excess {
                segments.removeFirst()
                bytes -= utf8.count
                removedBytes += utf8.count
                continue
            }
            var cut = excess
            if let newline = utf8[cut...].firstIndex(of: 0x0A) {
                cut = newline + 1
            } else {
                while cut < utf8.count, utf8[cut] & 0xC0 == 0x80 { cut += 1 }
            }
            segments[0].text = String(decoding: utf8[cut...], as: UTF8.self)
            bytes -= cut
            removedBytes += cut
            if segments[0].text.isEmpty { segments.removeFirst() }
        }
    }
}

/// Where a run's events wait for the console, which takes them at most
/// every 50 ms so a tight `print` loop can't flood the screen. Pushed
/// from the interpreter's thread, drained on the main actor.
final class ConsoleInbox: @unchecked Sendable {
    private let lock = NSLock()
    private var events: [PythonConsoleEvent] = []

    func push(_ event: PythonConsoleEvent) {
        lock.withLock { events.append(event) }
    }

    func drain() -> [PythonConsoleEvent] {
        lock.withLock {
            defer { events.removeAll(keepingCapacity: true) }
            return events
        }
    }
}

/// What the shared interpreter is doing that another screen should know:
/// Python Lab and Bonsai Harness take turns on it, so while a Python Lab
/// script waits for an answer, Bonsai's runs wait too.
@MainActor
@Observable
public final class PythonActivity {
    public static let shared = PythonActivity()

    /// A Python Lab script is waiting for `input()`.
    public internal(set) var labWaitingForInput = false
    /// The project Bonsai Harness is working in while a task of the
    /// person's runs, which Python Lab shows read-only meanwhile (PL1e).
    public internal(set) var bonsaiProject: String?

    public init() {}
}
