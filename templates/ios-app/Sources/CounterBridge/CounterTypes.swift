@preconcurrency import ForgeFFI

// Swift-side value types for what crosses the bridge.
//
// The generated UniFFI types are not `Sendable`, so Swift 6 code cannot pass
// them between actors. These small copies are, and they keep SwiftUI code
// independent of the generated module. They carry data only: every rule about
// what the values may be lives in Rust.

/// The committed counter state.
public struct CounterState: Sendable, Equatable {
    public let count: Int64

    public init(count: Int64) {
        self.count = count
    }

    init(_ snapshot: CounterStateSnapshot) {
        self.init(count: snapshot.count)
    }
}

/// A short command. Each one is saved before it reports success.
public enum CounterAction: Sendable, Equatable {
    case increment
    case decrement
    case reset

    var ffi: ForgeActionKind {
        switch self {
        case .increment: return .increment
        case .decrement: return .decrement
        case .reset: return .reset
        }
    }
}

/// Whether the instance found saved data when it opened.
public enum StartupKind: Sendable, Equatable {
    case freshInstall
    case restored

    init(_ kind: ForgeFFI.StartupKind) {
        switch kind {
        case .freshInstall: self = .freshInstall
        case .restored: self = .restored
        }
    }
}

/// Progress of a long-running operation.
public struct OperationProgress: Sendable, Equatable {
    public let operationID: UInt64
    public let completed: UInt32
    public let total: UInt32

    /// 0...1, for a `ProgressView`.
    public var fraction: Double {
        total == 0 ? 0 : Double(completed) / Double(total)
    }
}

/// How a long-running operation ended.
public enum OperationResult: Sendable, Equatable {
    case completed(CounterState)
    case cancelled
    case failed(CounterError)
}

/// Something that happened in Rust. Delivered only after it was saved.
public enum CounterEvent: Sendable, Equatable {
    /// The committed count changed, whoever changed it.
    case stateChanged(CounterState)
    case operationProgress(OperationProgress)
    /// Exactly one per operation.
    case operationFinished(operationID: UInt64, result: OperationResult)
    /// This subscriber fell behind and `count` older events were discarded.
    /// Re-read `CounterClient.currentState()` instead of relying on them.
    case eventsDropped(count: UInt64)

    init(_ event: ForgeFFI.CounterEvent) {
        switch event {
        case let .stateChanged(snapshot):
            self = .stateChanged(CounterState(snapshot))
        case let .operationProgress(operationId, completed, total):
            self = .operationProgress(
                OperationProgress(operationID: operationId, completed: completed, total: total))
        case let .operationFinished(operationId, outcome):
            let result: OperationResult
            switch outcome {
            case let .completed(snapshot): result = .completed(CounterState(snapshot))
            case .cancelled: result = .cancelled
            case let .failed(error): result = .failed(CounterError(error))
            }
            self = .operationFinished(operationID: operationId, result: result)
        case let .eventsDropped(count):
            self = .eventsDropped(count: count)
        }
    }
}
