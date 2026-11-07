@preconcurrency import ForgeFFI
import Foundation

/// Presentation state for the notes screen.
///
/// Holds no app rules: every change goes to the Rust `AppCore` (your code in
/// `rust/src/lib.rs`), and the list shown comes from Rust, through a
/// command's result or a `.notesChanged` event. Every call is awaited, so the
/// main actor is never blocked; Rust does slow work on its own threads.
@MainActor
public final class NotesModel: ObservableObject {
    @Published public private(set) var notes: [Note] = []
    @Published public var draft = ""
    @Published public var url = "https://example.com"
    @Published public private(set) var urlResult: String?
    @Published public private(set) var isCheckingURL = false
    @Published public var errorMessage: String?
    /// True until the first open attempt finishes.
    @Published public private(set) var isLoading = true
    /// Set when the app could not be opened (for example unreadable data).
    @Published public private(set) var openError: String?
    /// The lifecycle phase last reported to Rust.
    @Published public private(set) var lifecyclePhase: AppPhase = .launching

    public private(set) var core: AppCore?
    private let storageDirectory: String?
    private var eventTask: Task<Void, Never>?
    private var urlCheck: Cancellation?
    private var events: AppEvents?

    /// - Parameter storageDirectory: where Rust keeps `notes.json`. Defaults
    ///   to the app's Application Support folder; tests pass a temporary one.
    ///   Nothing is opened until `open()`.
    public init(storageDirectory: String? = nil) {
        self.storageDirectory = storageDirectory
    }

    /// Opens the Rust core and starts listening for its events. Call it from
    /// `.task`; calling it again while open does nothing, so a view that
    /// reappears never creates a second instance or subscription.
    public func open() async {
        guard core == nil else { return }
        isLoading = true
        defer { isLoading = false }
        openError = nil
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
            // A phase reported before Rust was open still counts.
            if lifecyclePhase != .launching { core.setPhase(phase: lifecyclePhase) }
        } catch {
            openError = Self.describe(error)
        }
    }

    /// Ends the event stream and shuts the Rust core down. Saved notes are
    /// already on disk.
    public func close() {
        events?.cancel()
        eventTask?.cancel()
        core?.shutdown()
        core = nil
        events = nil
    }

    /// Tells Rust where the app is in its lifecycle. `ContentView` calls it
    /// through `.forgeLifecycle`. Rust stops on-screen work in the
    /// background and sends `.phaseChanged(resumed: true)` on the way back.
    /// `.terminated` shuts the core down, like `close()`.
    public func setPhase(_ phase: AppPhase) {
        lifecyclePhase = phase
        if phase == .terminated {
            close()
        } else {
            core?.setPhase(phase: phase)
        }
    }

    public func addNote() async {
        guard let core else { return }
        errorMessage = nil
        do {
            _ = try await core.addNote(text: draft)
            draft = ""
            // The list updates from the `.notesChanged` event; reading it
            // here as well makes the change visible without waiting for it.
            notes = core.notes()
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    public func delete(_ note: Note) async {
        guard let core else { return }
        errorMessage = nil
        do {
            try await core.deleteNote(id: note.id)
            notes = core.notes()
        } catch {
            errorMessage = Self.describe(error)
        }
    }

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
                case .phaseChanged(_, let resumed):
                    // Back from the background: re-read what Rust holds. A
                    // real app would also restart work the background stopped.
                    if resumed, let core = self.core { self.notes = core.notes() }
                }
            }
        }
    }

    /// Text for the screen. Replace with your own copy; `reason` fields are
    /// diagnostics from Rust.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? AppError else { return String(describing: error) }
        switch error {
        case .InvalidInput(let reason): return "Check your input: \(reason)"
        case .NotFound: return "That note no longer exists."
        case .Storage(let reason): return "Could not save: \(reason)"
        case .Offline: return "You appear to be offline."
        case .Certificate(let reason): return "The server's certificate was rejected: \(reason)"
        case .Timeout: return "The request timed out."
        case .Network(let reason): return "Network error: \(reason)"
        case .Cancelled: return "Cancelled."
        case .Interrupted: return "Stopped when the app went to the background. Try again."
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
