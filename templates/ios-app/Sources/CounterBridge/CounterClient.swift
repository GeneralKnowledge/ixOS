import Foundation
@preconcurrency import ForgeFFI

/// The Swift face of one Rust counter instance.
///
/// `CounterClient` owns the Rust instance: it lives as long as the client
/// does (plus any call still in flight), and is shut down when the last
/// reference goes away or `shutdown()` is called. Share one client between
/// the views and models that need it; open one per storage folder.
///
/// Concurrency contract (see docs/ffi-policy.md):
/// - Every method is safe to call from any task or actor, including the main
///   actor. Nothing here blocks: commands run on the Rust instance's own
///   command thread, and queries read the last committed state.
/// - Commands run one at a time, in the order Rust receives them, and resolve
///   only after their change has been saved.
/// - A short command, once sent, always completes, even if the calling task is
///   cancelled. `bulkAdd` is cancellable up to its commit point.
public final class CounterClient: @unchecked Sendable {
    // @unchecked: the generated class is not marked Sendable, but the Rust
    // object behind it is (`Send + Sync`), and the class holds nothing else.
    private let app: ForgeCounterApp

    /// Whether saved data was found when the instance opened.
    public let startupKind: StartupKind

    private init(app: ForgeCounterApp) throws {
        self.app = app
        self.startupKind = StartupKind(try bridged { try app.startupKind() })
    }

    /// Opens the counter stored in `storageDirectory` without blocking the
    /// caller. With `recoverCorruptData`, corrupt data (and only corrupt data)
    /// is replaced with a fresh counter.
    public static func open(
        storageDirectory: String,
        recoverCorruptData: Bool = false
    ) async throws -> CounterClient {
        let app = try await bridgedAsync {
            try await openCounterApp(
                storageDirectory: storageDirectory,
                mode: recoverCorruptData ? .recoverCorruptData : .normal)
        }
        return try CounterClient(app: app)
    }

    /// Diagnostic greeting from the Rust core.
    public static var greeting: String { forgeGreeting() }

    // MARK: Queries (never wait for disk)

    /// The last committed state. Safe on the main actor.
    public func currentState() throws -> CounterState {
        CounterState(try bridged { try app.snapshot() })
    }

    /// The running long-running operation, if any.
    public var activeOperation: OperationProgress? {
        app.activeOperation().map {
            OperationProgress(operationID: $0.operationId, completed: $0.completed, total: $0.total)
        }
    }

    public var isShutDown: Bool { app.isShutDown() }

    /// Live event subscriptions (diagnostics and tests).
    public var subscriptionCount: Int { Int(app.subscriptionCount()) }

    // MARK: Commands

    /// Applies `action` and returns the committed state.
    ///
    /// If the task is already cancelled, nothing is sent and this throws
    /// `CancellationError`. Once sent, the command completes and is saved
    /// whatever happens to the task.
    public func apply(_ action: CounterAction) async throws -> CounterState {
        try Task.checkCancellation()
        let outcome = try await bridgedAsync { try await app.applyAction(action: action.ffi) }
        return CounterState(outcome.snapshot)
    }

    /// Adds `amount` as a long-running, all-or-nothing operation (a stand-in
    /// for an import) and returns the committed state.
    ///
    /// Progress arrives as `.operationProgress` events. Cancelling the calling
    /// task cancels the Rust operation: if that happens before its commit
    /// point, nothing changes and this throws `CancellationError`; if the
    /// operation had already committed, this returns the committed state.
    public func bulkAdd(_ amount: UInt32, stepDelay: Duration = .milliseconds(100)) async throws
        -> CounterState
    {
        try Task.checkCancellation()
        let operation = try bridged {
            try app.startBulkAdd(amount: amount, stepDelayMs: Self.milliseconds(stepDelay))
        }
        let handle = UncheckedSendable(operation)
        let snapshot = try await withTaskCancellationHandler {
            try await bridgedAsync { try await handle.value.result() }
        } onCancel: {
            handle.value.cancel()
        }
        return CounterState(snapshot)
    }

    // MARK: Events

    /// Subscribes to Rust events. Subscribe *before* reading `currentState()`
    /// so no change can slip between the two.
    ///
    /// Iterate the result once, from one task. The subscription ends when the
    /// iterating task is cancelled, when the sequence is released, or when the
    /// instance shuts down (the loop then simply ends).
    public func events() throws -> CounterEvents {
        CounterEvents(subscription: try bridged { try app.subscribe() })
    }

    // MARK: Lifecycle

    /// Shuts the Rust instance down and waits until it has stopped: commands
    /// already sent finish and are saved, a running `bulkAdd` is cancelled,
    /// and every event sequence ends. Later commands throw `.closed`.
    /// Queries keep returning the last committed state. Idempotent.
    public func shutdown() async {
        await app.shutdown()
    }

    private static func milliseconds(_ duration: Duration) -> UInt32 {
        let (seconds, attoseconds) = duration.components
        let ms = Double(seconds) * 1_000 + Double(attoseconds) / 1e15
        return UInt32(clamping: Int64(max(0, ms.rounded())))
    }
}

/// Rust events as an `AsyncSequence`.
///
/// Each `next()` awaits the next event from Rust, so a slow consumer slows
/// delivery instead of piling up events in Swift: Rust keeps up to 64 per
/// subscriber and reports overflow with `.eventsDropped`. (An `AsyncStream`
/// fed by a pump task would hide that backpressure behind its own buffer.)
public struct CounterEvents: AsyncSequence, Sendable {
    public typealias Element = CounterEvent

    private let subscription: UncheckedSendable<CounterEventSubscription>

    init(subscription: CounterEventSubscription) {
        self.subscription = UncheckedSendable(subscription)
    }

    /// Ends the subscription now. A pending `next()` returns `nil`.
    public func cancel() {
        subscription.value.cancel()
    }

    public func makeAsyncIterator() -> AsyncIterator {
        AsyncIterator(subscription: subscription)
    }

    public struct AsyncIterator: AsyncIteratorProtocol, Sendable {
        let subscription: UncheckedSendable<CounterEventSubscription>

        public mutating func next() async -> CounterEvent? {
            // UniFFI 0.28 does not forward Swift task cancellation to Rust, so
            // do it here: cancelling the subscription ends the pending `next()`.
            let subscription = self.subscription
            if Task.isCancelled {
                subscription.value.cancel()
                return nil
            }
            let event = await withTaskCancellationHandler {
                await subscription.value.next()
            } onCancel: {
                subscription.value.cancel()
            }
            return event.map(CounterEvent.init)
        }
    }
}

/// Carries a generated (non-`Sendable`) UniFFI object across concurrency
/// domains. Sound because the Rust objects behind the generated classes are
/// `Send + Sync` and the classes hold nothing but the pointer.
struct UncheckedSendable<Value>: @unchecked Sendable {
    let value: Value

    init(_ value: Value) {
        self.value = value
    }
}
