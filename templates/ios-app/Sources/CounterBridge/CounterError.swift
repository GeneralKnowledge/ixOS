@preconcurrency import ForgeFFI

/// Errors from the counter bridge, translated from Rust's `ForgeAppError`.
///
/// Cancellation is never a `CounterError`: a cancelled operation throws Swift's
/// `CancellationError`, so callers can ignore it the usual way.
public enum CounterError: Error, Sendable, Equatable {
    // Domain validation: the request was refused and nothing changed.
    case belowMinimum
    case aboveMaximum
    case invalidInput(detail: String)
    // Persistence.
    case saveFailed(detail: String)
    case readFailed(detail: String)
    case storageUnavailable
    /// Saved data is unreadable. The host may offer to reset it.
    case corruptData(detail: String)
    /// Saved by a newer app version. Valid data: never offer a reset.
    case newerDataVersion(found: UInt32, supported: UInt32)
    // Lifecycle and resources.
    /// The Rust instance has been shut down; open a new one.
    case closed
    /// Another long-running operation is already running.
    case busy
    // Failures that are bugs, or outside Rust.
    case internalFailure(detail: String)
    /// A Swift-side platform service failed (for example creating the
    /// storage folder).
    case platform(detail: String)

    public enum Category: Sendable {
        case domain, persistence, corruptData, newerData, lifecycle, internalFailure, platform
    }

    public var category: Category {
        switch self {
        case .belowMinimum, .aboveMaximum, .invalidInput: return .domain
        case .saveFailed, .readFailed, .storageUnavailable: return .persistence
        case .corruptData: return .corruptData
        case .newerDataVersion: return .newerData
        case .closed, .busy: return .lifecycle
        case .internalFailure: return .internalFailure
        case .platform: return .platform
        }
    }

    /// Short text suitable for showing to the user. Diagnostic details are in
    /// `debugDescription`, not here.
    public var userMessage: String {
        switch self {
        case .belowMinimum: return "Count cannot go below zero."
        case .aboveMaximum: return "Count cannot exceed one million."
        case .invalidInput: return "That value is not allowed."
        case .saveFailed: return "Could not save counter state."
        case .readFailed: return "Could not read saved data. Check storage access and retry."
        case .storageUnavailable: return "Storage folder is unavailable. Retry after fixing permissions."
        case .corruptData: return "Stored counter data is invalid. You can reset storage or retry opening."
        case .newerDataVersion:
            return "Your counter was saved by a newer version of this app. Update the app to open it; your data has not been changed."
        case .closed: return "The counter has been closed. Reopen it to continue."
        case .busy: return "Another operation is still running."
        case .internalFailure: return "Internal error in Rust core."
        case .platform: return "The app could not prepare its storage."
        }
    }

    init(_ error: ForgeAppError) {
        switch error {
        case .WouldUnderflow: self = .belowMinimum
        case .WouldOverflow: self = .aboveMaximum
        case let .InvalidInput(reason): self = .invalidInput(detail: reason)
        case let .PersistenceFailed(reason): self = .saveFailed(detail: reason)
        case let .CorruptPersistence(reason): self = .corruptData(detail: reason)
        case let .UnsupportedSchemaVersion(found, supported):
            self = .newerDataVersion(found: found, supported: supported)
        case let .PersistenceReadFailed(reason): self = .readFailed(detail: reason)
        case .StorageDirectoryUnavailable: self = .storageUnavailable
        // Callers translate `.Cancelled` to `CancellationError` first; this
        // case only appears inside an event's failure outcome.
        case .Cancelled: self = .internalFailure(detail: "unexpected cancellation")
        case .ShutDown: self = .closed
        case .OperationInProgress: self = .busy
        case let .InternalFailure(reason): self = .internalFailure(detail: reason)
        }
    }

    /// Maps anything thrown by the generated bindings to what the bridge
    /// promises: `CancellationError` for cancellation, otherwise `CounterError`.
    static func translate(_ error: any Error) -> any Error {
        switch error {
        case ForgeAppError.Cancelled:
            return CancellationError()
        case let error as ForgeAppError:
            return CounterError(error)
        case is CancellationError, is CounterError:
            return error
        default:
            return CounterError.internalFailure(detail: String(describing: error))
        }
    }
}

extension CounterError: CustomDebugStringConvertible {
    public var debugDescription: String {
        switch self {
        case let .invalidInput(detail), let .saveFailed(detail), let .readFailed(detail),
            let .corruptData(detail), let .internalFailure(detail), let .platform(detail):
            return "CounterError.\(category): \(detail)"
        case let .newerDataVersion(found, supported):
            return "CounterError.newerData: schema \(found), this build supports up to \(supported)"
        default:
            return "CounterError.\(category): \(userMessage)"
        }
    }
}

/// Runs a bridge call and translates its error.
func bridged<T>(_ body: () throws -> T) throws -> T {
    do {
        return try body()
    } catch {
        throw CounterError.translate(error)
    }
}

/// Async form of `bridged`.
func bridgedAsync<T>(_ body: () async throws -> T) async throws -> T {
    do {
        return try await body()
    } catch {
        throw CounterError.translate(error)
    }
}
