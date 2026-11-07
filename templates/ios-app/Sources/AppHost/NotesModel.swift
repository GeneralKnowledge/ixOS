@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The app's presentation state: what the screens show, and the actions they
/// can take. It holds no app rules. Every change goes to the Rust `AppCore`
/// (your code in `rust/src/lib.rs`), and the notes shown come from Rust,
/// through a command's result or a `.notesChanged` event.
///
/// Views read its properties directly (`@Observable` tracks what each view
/// uses) and call its `async` methods from `Task { }` or `.task`. Rust does
/// slow work on its own threads, so the main actor never blocks.
@Observable
@MainActor
public final class NotesModel {
    /// Where opening the Rust core has got to. The root view shows a
    /// spinner, the app, or a recovery screen from this.
    public enum Phase: Equatable {
        case loading
        case ready
        case failed(String)
    }

    public private(set) var phase: Phase = .loading
    /// Every note, newest first, as Rust last reported it.
    public private(set) var notes: [Note] = []
    /// The search field's text. `visibleNotes` filters by it.
    public var searchText = ""
    /// A failed action, for an alert. The view clears it when dismissed.
    public var actionError: String?
    /// Increases after each successful save; views use it as a haptic trigger.
    public private(set) var saveCount = 0

    // Network check (Settings screen): an async Rust request that the user,
    // or the screen closing, can cancel part way.
    public var url = "https://example.com"
    public private(set) var urlResult: String?
    public private(set) var isCheckingURL = false

    public private(set) var core: AppCore?
    private let storageDirectory: String?
    @ObservationIgnored private var eventTask: Task<Void, Never>?
    @ObservationIgnored private var events: AppEvents?
    @ObservationIgnored private var urlCheck: Cancellation?

    /// - Parameter storageDirectory: where Rust keeps `notes.json`. Defaults
    ///   to the app's Application Support folder; tests pass a temporary one.
    ///   Nothing is opened until `open()`.
    public init(storageDirectory: String? = nil) {
        self.storageDirectory = storageDirectory
    }

    /// The notes to list: all of them, or Rust's matches for `searchText`.
    public var visibleNotes: [Note] {
        guard let core, !searchText.trimmingCharacters(in: .whitespaces).isEmpty else { return notes }
        // Reading `notes` makes views that show this refresh when it changes.
        _ = notes
        return core.searchNotes(query: searchText)
    }

    /// The current version of a note, or nil once it has been deleted.
    public func note(id: UInt64) -> Note? {
        notes.first { $0.id == id }
    }

    // MARK: Lifecycle

    /// Opens the Rust core and starts listening for its events. Call it from
    /// `.task`; calling it again while open does nothing, so a view that
    /// reappears never creates a second instance or subscription.
    public func open() async {
        guard core == nil else { return }
        phase = .loading
        do {
            let directory = try storageDirectory ?? Self.applicationSupportDirectory()
            // Opening reads a file: do it off the main actor.
            let core = try await Task.detached { try AppCore(storageDirectory: directory) }.value
            // Subscribe before reading, so no change is missed in between.
            let events = try core.events()
            self.core = core
            self.events = events
            notes = core.notes()
            listen(to: events)
            phase = .ready
        } catch {
            phase = .failed(Self.describe(error))
        }
    }

    /// Ends the event stream and shuts the Rust core down, which also stops
    /// any request in flight. Saved notes are already on disk.
    public func close() {
        events?.cancel()
        eventTask?.cancel()
        core?.shutdown()
        core = nil
        events = nil
    }

    // MARK: Notes

    /// Adds a note. Throws Rust's `AppError` so the compose sheet can stay
    /// open with the text intact and present the error itself.
    public func add(text: String) async throws {
        guard let core else { throw AppError.ShutDown }
        _ = try await core.addNote(text: text)
        notes = core.notes()
        saveCount += 1
    }

    /// Saves new text for a note. Returns false (and sets `actionError`) on failure.
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
        guard let core, !isCheckingURL else { return }
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
    /// `.notesChanged` event carries the same list moments later). A failure
    /// goes to `actionError`, which the list presents as an alert.
    @discardableResult
    private func perform(_ command: (AppCore) async throws -> Void) async -> Bool {
        guard let core else { return false }
        do {
            try await command(core)
            notes = core.notes()
            saveCount += 1
            return true
        } catch {
            actionError = Self.describe(error)
            return false
        }
    }

    private func listen(to events: AppEvents) {
        eventTask = Task { [weak self] in
            while let event = await events.next() {
                guard let self else { return }
                switch event {
                case .notesChanged(let notes):
                    self.notes = notes
                case .eventsDropped:
                    // Missed some events: the current list is the truth.
                    if let core = self.core { self.notes = core.notes() }
                }
            }
        }
    }

    /// Text for the screen. Replace with your own copy; `reason` fields are
    /// diagnostics from Rust.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return error.localizedDescription }
        switch error {
        case .InvalidInput(let reason): return "Check your input: \(reason)"
        case .NotFound: return "That note no longer exists."
        case .Storage(let reason): return "Your notes could not be saved: \(reason)"
        case .Offline: return "You appear to be offline."
        case .Certificate(let reason): return "The server's certificate was rejected: \(reason)"
        case .Timeout: return "The request timed out."
        case .Network(let reason): return "Network error: \(reason)"
        case .Cancelled: return "Cancelled."
        case .ShutDown: return "The app is closing."
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
