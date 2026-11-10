@preconcurrency import ForgeFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation

/// Swift log records go into the same log as Rust's: the system log
/// (`forge device logs`), recent records and the `forge device` live stream.
private let log = ForgeLog("Notes")

/// The app's presentation state: what the screens show, and the actions they
/// can take. It holds no app rules. Every change goes to the Rust `AppCore`
/// (your code in `rust/src/lib.rs`), and the notes shown come from Rust.
///
/// ForgeKit's `ForgeHost` owns the core for the life of the process (see
/// AppHost.swift); this model is given it once the core is open. When Rust
/// commits a change, the host's `changes` moves and the view calls
/// `reload()`.
///
/// Views read its properties directly (`@Observable` tracks what each view
/// uses) and call its `async` methods from `Task { }` or `.task`. Rust does
/// slow work on its own threads, so the main actor never blocks.
@Observable
@MainActor
public final class NotesModel {
    /// Every note, newest first, as Rust last reported it.
    public private(set) var notes: [Note] = []
    /// The search field's text. `visibleNotes` filters by it.
    public var searchText = ""
    /// A failed action, as a ForgeAppKit alert value. The list presents it
    /// with `.forgeAlert`, which clears it when dismissed.
    public var alert: ForgeAlert<Never>?
    /// Increases after each successful save; views use it as a haptic trigger.
    public private(set) var saveCount = 0

    // Network check (Settings screen): an async Rust request that the user,
    // or the screen closing, can cancel part way.
    public var url = "https://example.com"
    public private(set) var urlResult: String?
    public private(set) var isCheckingURL = false

    public let core: AppCore
    @ObservationIgnored private var urlCheck: Cancellation?

    public init(core: AppCore) {
        self.core = core
        notes = core.notes()
    }

    /// Re-reads the notes from Rust. Cheap: Rust hands over its published
    /// list without waiting for any command.
    public func reload() {
        notes = core.notes()
    }

    /// The notes to list: all of them, or Rust's matches for `searchText`.
    public var visibleNotes: [Note] {
        guard !searchText.trimmingCharacters(in: .whitespaces).isEmpty else { return notes }
        // Reading `notes` makes views that show this refresh when it changes.
        _ = notes
        return core.searchNotes(query: searchText)
    }

    /// The current version of a note, or nil once it has been deleted.
    public func note(id: UInt64) -> Note? {
        notes.first { $0.id == id }
    }

    // MARK: Notes

    /// Adds a note. Throws Rust's `AppError` so the compose sheet can stay
    /// open with the text intact and present the error itself.
    public func add(text: String) async throws {
        _ = try await core.addNote(text: text)
        notes = core.notes()
        saveCount += 1
    }

    /// Saves new text for a note. Returns false (and sets `alert`) on failure.
    @discardableResult
    public func update(_ note: Note, text: String) async -> Bool {
        guard text != note.text else { return true }
        return await perform { core in
            _ = try await core.updateNote(id: note.id, text: text)
        }
    }

    public func delete(_ note: Note) async {
        await perform { core in
            try await core.deleteNote(id: note.id)
        }
    }

    // MARK: Network check

    /// Fetches `url` from Rust (HTTPS certificates are checked by the OS).
    /// Cancelling the calling task, or `cancelURLCheck()`, stops the request
    /// in Rust at once, even while it waits for a slow server.
    public func checkURL() async {
        guard !isCheckingURL else { return }
        isCheckingURL = true
        defer {
            isCheckingURL = false
            urlCheck = nil
        }
        urlResult = nil
        // Cancelling a Swift Task does not reach Rust by itself (UniFFI 0.32),
        // so forward it through a Cancellation.
        let cancellation = Cancellation()
        urlCheck = cancellation
        do {
            let status = try await withTaskCancellationHandler {
                try await core.checkUrl(url: url, cancellation: cancellation)
            } onCancel: {
                cancellation.cancel()
            }
            urlResult = "HTTP \(status)"
        } catch {
            urlResult = Self.describe(error)
        }
    }

    /// Stops the URL check in flight, if any.
    public func cancelURLCheck() {
        urlCheck?.cancel()
    }

    // MARK: Private

    /// Runs one Rust command, then shows Rust's list straight away (the
    /// host's `changes` moves moments later, and `reload()` reads the same
    /// list). A failure goes to `alert`, which the list presents.
    @discardableResult
    private func perform(_ command: (AppCore) async throws -> Void) async -> Bool {
        do {
            try await command(core)
            notes = core.notes()
            saveCount += 1
            return true
        } catch {
            log.warn("command failed: \(error)")
            alert = .failure(error)
            return false
        }
    }

    /// Text for the screen. Replace with your own copy; `reason` fields are
    /// diagnostics from Rust.
    nonisolated public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return error.localizedDescription }
        switch error {
        case .InvalidInput(let reason): return "Check your input: \(reason)"
        case .NotFound: return "That note no longer exists."
        case .Storage(let reason): return "Your notes couldn't be read or saved (\(reason))."
        case .InUse: return "Your notes are open in another copy of the app."
        case .Offline: return "You appear to be offline."
        case .Certificate(let reason): return "The server's certificate was rejected: \(reason)"
        case .Timeout: return "The request timed out."
        case .Network(let reason): return "Network error: \(reason)"
        case .Cancelled: return "Cancelled."
        case .Interrupted: return "Stopped when the app went to the background. Try again."
        case .ShutDown: return "The app is closing."
        }
    }
}

/// Every alert and failed save shows `NotesModel.describe`'s words
/// (ForgeAppKit). A cancelled request is not a failure: nothing is shown.
extension AppError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Offline, .Timeout, .Network, .Interrupted: retryable = true
        default: retryable = false
        }
        return ForgeFailure(title: "Something Went Wrong", message: NotesModel.describe(self), isRetryable: retryable)
    }

    public var isCancellation: Bool {
        if case .Cancelled = self { return true }
        return false
    }
}
