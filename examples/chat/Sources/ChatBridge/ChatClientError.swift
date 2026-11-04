import Foundation
@preconcurrency import ForgeFFI

/// Errors from the chat bridge, translated from Rust's `ChatError`.
///
/// `Cancelled` (the app shut down mid-send) arrives as Swift's
/// `CancellationError`, never as a `ChatClientError`.
public enum ChatClientError: Error, Sendable, Equatable {
    /// The server could not be reached, the connection broke, or it timed out.
    case network(detail: String)
    /// The server answered with an error status.
    case server(status: UInt16, detail: String)
    /// The server answered with something this build cannot read.
    case badResponse(detail: String)
    case notFound(conversationId: String)
    /// This device cannot do what was asked.
    case unsupported(detail: String)
    /// Refused before anything was sent (empty message, bad server URL).
    case invalidInput(detail: String)
    /// The stored chat data cannot be used.
    case storage(detail: String)
    /// The client has been shut down.
    case closed
    case internalFailure(detail: String)

    /// Short text suitable for showing to the user.
    public var userMessage: String {
        switch self {
        case .network: return "Can't reach the chat server. Is it running?"
        case let .server(status, _) where status >= 500:
            return "The chat server is having problems. Try again."
        case .server: return "The chat server refused the request."
        case .badResponse: return "The chat server sent something this app can't read."
        case .notFound: return "That conversation no longer exists."
        case .unsupported: return "This device can't do that yet."
        case .invalidInput: return "That can't be sent."
        case .storage: return "Saved chats can't be read by this version of the app."
        case .closed: return "Chat has been closed. Reopen the app to continue."
        case .internalFailure: return "Something went wrong in the chat engine."
        }
    }

    init(_ error: ChatError) {
        switch error {
        case let .Network(reason): self = .network(detail: reason)
        case let .Http(status, reason): self = .server(status: status, detail: reason)
        case let .BadResponse(reason): self = .badResponse(detail: reason)
        // Callers translate `.Cancelled` to `CancellationError` first.
        case .Cancelled: self = .internalFailure(detail: "unexpected cancellation")
        case let .NotFound(conversationId): self = .notFound(conversationId: conversationId)
        case let .UnsupportedOnHost(detail): self = .unsupported(detail: detail)
        case let .InvalidInput(reason): self = .invalidInput(detail: reason)
        case let .Storage(reason): self = .storage(detail: reason)
        case .ShutDown: self = .closed
        case let .Internal(reason): self = .internalFailure(detail: reason)
        }
    }

    /// Maps anything thrown by the generated bindings to what the bridge
    /// promises: `CancellationError` or `ChatClientError`.
    static func translate(_ error: any Error) -> any Error {
        switch error {
        case ChatError.Cancelled:
            return CancellationError()
        case let error as ChatError:
            return ChatClientError(error)
        case is CancellationError, is ChatClientError:
            return error
        default:
            return ChatClientError.internalFailure(detail: String(describing: error))
        }
    }
}

func bridged<T>(_ body: () throws -> T) throws -> T {
    do {
        return try body()
    } catch {
        throw ChatClientError.translate(error)
    }
}

func bridgedAsync<T>(_ body: () async throws -> T) async throws -> T {
    do {
        return try await body()
    } catch {
        throw ChatClientError.translate(error)
    }
}
