import Foundation
@preconcurrency import ForgeFFI

/// Errors from the weather bridge, translated from Rust's `WeatherError`.
///
/// Cancellation is never a `WeatherClientError`: a cancelled refresh or
/// search throws Swift's `CancellationError`.
public enum WeatherClientError: Error, Sendable, Equatable {
    /// The request was refused before anything was sent.
    case invalidInput(detail: String)
    /// No connection, or the service could not be reached.
    case offline(detail: String)
    case timedOut
    /// The service answered with an error status.
    case server(status: UInt16, reason: String?)
    /// The service answered with something this build cannot read.
    case badResponse(detail: String)
    /// TLS or another transport failure.
    case network(detail: String)
    /// The client has been shut down.
    case closed
    case internalFailure(detail: String)

    /// Short text suitable for showing to the user.
    public var userMessage: String {
        switch self {
        case .invalidInput: return "That location can't be used."
        case .offline: return "You're offline. Connect to the internet and try again."
        case .timedOut: return "The weather service took too long to answer. Try again."
        case let .server(status, _) where status >= 500:
            return "The weather service is having problems. Try again later."
        case .server: return "The weather service refused the request."
        case .badResponse: return "The weather service sent data this app can't read."
        case .network: return "A secure connection to the weather service failed."
        case .closed: return "Weather has been closed. Reopen the app to continue."
        case .internalFailure: return "Something went wrong in the weather engine."
        }
    }

    init(_ error: WeatherError) {
        switch error {
        case let .InvalidInput(reason): self = .invalidInput(detail: reason)
        case let .Offline(reason): self = .offline(detail: reason)
        case .Timeout: self = .timedOut
        case let .Server(status, reason): self = .server(status: status, reason: reason)
        case let .BadResponse(reason): self = .badResponse(detail: reason)
        case let .Network(reason): self = .network(detail: reason)
        // Callers translate `.Cancelled` to `CancellationError` first.
        case .Cancelled: self = .internalFailure(detail: "unexpected cancellation")
        case .ShutDown: self = .closed
        case let .Internal(reason): self = .internalFailure(detail: reason)
        }
    }

    /// Maps anything thrown by the generated bindings to what the bridge
    /// promises: `CancellationError` for cancellation, otherwise
    /// `WeatherClientError`.
    static func translate(_ error: any Error) -> any Error {
        switch error {
        case WeatherError.Cancelled:
            return CancellationError()
        case let error as WeatherError:
            return WeatherClientError(error)
        case is CancellationError, is WeatherClientError:
            return error
        default:
            return WeatherClientError.internalFailure(detail: String(describing: error))
        }
    }
}

func bridged<T>(_ body: () throws -> T) throws -> T {
    do {
        return try body()
    } catch {
        throw WeatherClientError.translate(error)
    }
}

func bridgedAsync<T>(_ body: () async throws -> T) async throws -> T {
    do {
        return try await body()
    } catch {
        throw WeatherClientError.translate(error)
    }
}
