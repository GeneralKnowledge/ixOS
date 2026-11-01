import CounterBridge
import Foundation

/// Presentation state for the counter screen.
///
/// Holds no counter rules: every change goes to Rust through `CounterClient`,
/// and the displayed count comes from Rust (a command's result, or a
/// `.stateChanged` event). All work is awaited, so the main actor is never
/// blocked; slow work runs on the Rust side.
@MainActor
public final class CounterViewModel: ObservableObject {
    @Published public private(set) var count: Int64 = 0
    @Published public private(set) var startupLabel: String = ""
    @Published public var errorMessage: String?
    /// True until the first open attempt finishes.
    @Published public private(set) var isLoading = true
    @Published public private(set) var isBlocked = false
    /// True only when stored data is corrupt. Data saved by a newer app
    /// version is valid, so it is never offered for reset.
    @Published public private(set) var canRecoverFromCorruption = false
    /// Progress of the running bulk add (0...1), or nil when none is running.
    @Published public private(set) var bulkAddProgress: Double?
    @Published public private(set) var lastOperationMessage: String?

    public private(set) var client: CounterClient?
    private let storageDirectory: String?
    private var isOpening = false
    private var eventTask: Task<Void, Never>?
    private var bulkAddTask: Task<Void, Never>?

    /// - Parameter storageDirectory: where Rust keeps its snapshot. Defaults to
    ///   the app's Application Support folder; tests pass a temporary folder.
    ///   Nothing is opened until `open()`.
    public init(storageDirectory: String? = nil) {
        self.storageDirectory = storageDirectory
    }

    /// Opens the Rust instance and starts listening for its events. Call it
    /// from `.task`; calling it again while open (or opening) does nothing, so
    /// a view that reappears never creates a second subscription.
    public func open() async {
        guard client == nil, !isOpening else { return }
        await attach(recoverCorruptData: false)
    }

    public func retryOpen() async {
        guard client == nil, !isOpening else { return }
        await attach(recoverCorruptData: false)
    }

    public func recoverFromCorruption() async {
        guard client == nil, !isOpening else { return }
        await attach(recoverCorruptData: true)
    }

    /// Shuts the Rust instance down (saved state is already on disk).
    public func close() async {
        bulkAddTask?.cancel()
        eventTask?.cancel()
        let client = self.client
        self.client = nil
        await client?.shutdown()
    }

    public func increment() async { await apply(.increment) }
    public func decrement() async { await apply(.decrement) }
    public func reset() async { await apply(.reset) }

    /// Starts a bulk add. Progress and completion arrive as Rust events; the
    /// returned task (also used by `cancelBulkAdd()`) finishes when it ends.
    ///
    /// If this view model goes away first, the operation still runs to
    /// completion in Rust and its result is saved; nothing here is updated.
    @discardableResult
    public func startBulkAdd(amount: UInt32 = 100, stepDelay: Duration = .milliseconds(100))
        -> Task<Void, Never>?
    {
        guard let client, bulkAddTask == nil else { return nil }
        errorMessage = nil
        lastOperationMessage = nil
        bulkAddProgress = 0
        let task = Task { [weak self] in
            do {
                let state = try await client.bulkAdd(amount, stepDelay: stepDelay)
                self?.show(state)
                self?.lastOperationMessage = "Added \(amount)."
            } catch is CancellationError {
                self?.lastOperationMessage = "Bulk add cancelled; nothing was added."
            } catch {
                self?.showInline(error)
            }
            self?.bulkAddProgress = nil
            self?.bulkAddTask = nil
        }
        bulkAddTask = task
        return task
    }

    public func cancelBulkAdd() {
        bulkAddTask?.cancel()
    }

    public var isBulkAddRunning: Bool { bulkAddProgress != nil }

    // MARK: Private

    private func attach(recoverCorruptData: Bool) async {
        isOpening = true
        defer {
            isOpening = false
            isLoading = false
        }
        errorMessage = nil
        do {
            let directory = try storageDirectory ?? Self.applicationSupportDirectory()
            let client = try await CounterClient.open(
                storageDirectory: directory, recoverCorruptData: recoverCorruptData)
            // Subscribe before reading state, so no change is missed in between.
            let events = try client.events()
            let state = try client.currentState()
            self.client = client
            isBlocked = false
            canRecoverFromCorruption = false
            show(state)
            switch (recoverCorruptData, client.startupKind) {
            case (true, .freshInstall): startupLabel = "Recovered — fresh counter"
            case (_, .freshInstall): startupLabel = "Fresh install"
            case (_, .restored): startupLabel = "Restored saved state"
            }
            listen(to: events, client: client)
        } catch {
            presentBlocked(error)
        }
    }

    private func listen(to events: CounterEvents, client: CounterClient) {
        eventTask?.cancel()
        // Weak, so a pending event wait never keeps this model alive. The loop
        // ends when the task is cancelled or the Rust instance shuts down.
        eventTask = Task { [weak self] in
            for await event in events {
                guard let self else { return }
                self.handle(event, from: client)
            }
        }
    }

    private func handle(_ event: CounterEvent, from client: CounterClient) {
        switch event {
        case let .stateChanged(state):
            show(state)
        case let .operationProgress(progress):
            if bulkAddTask != nil { bulkAddProgress = progress.fraction }
        case .operationFinished:
            // The bulk add task reports its own result.
            break
        case .eventsDropped:
            if let state = try? client.currentState() { show(state) }
        }
    }

    private func apply(_ action: CounterAction) async {
        guard let client else { return }
        errorMessage = nil
        do {
            show(try await client.apply(action))
        } catch {
            showInline(error)
        }
    }

    private func show(_ state: CounterState) {
        count = state.count
    }

    private func showInline(_ error: any Error) {
        errorMessage = (error as? CounterError)?.userMessage ?? error.localizedDescription
        if let state = try? client?.currentState() { show(state) }
    }

    private func presentBlocked(_ error: any Error) {
        isBlocked = true
        client = nil
        canRecoverFromCorruption = false
        guard let error = error as? CounterError else {
            errorMessage = error.localizedDescription
            return
        }
        if case .corruptData = error { canRecoverFromCorruption = true }
        errorMessage = error.userMessage
    }

    private static func applicationSupportDirectory() throws -> String {
        do {
            let base = try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("FORGE_APP_SUPPORT_DIR", isDirectory: true)
            try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
            return base.path
        } catch {
            throw CounterError.platform(detail: String(describing: error))
        }
    }
}
