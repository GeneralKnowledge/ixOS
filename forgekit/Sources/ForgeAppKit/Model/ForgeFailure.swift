// What went wrong, in words a person can act on. Foundation only: a view
// model (which imports no SwiftUI) makes it, and a Linux test checks it.

import Foundation

/// A failure as the app wants it shown: a short title, a sentence saying
/// what happened and what to do, and whether trying again might help.
///
///     ForgeFailure(title: "You're Offline", message: "Try again when you're connected.", isRetryable: true)
public struct ForgeFailure: Hashable, Sendable {
    public var title: String
    public var message: String
    public var isRetryable: Bool

    public init(title: String, message: String, isRetryable: Bool = false) {
        self.title = title
        self.message = message
        self.isRetryable = isRetryable
    }

    /// The failure for any error: the app's own words when the error is a
    /// `ForgeDescribedError`; for a URLSession error, words for being
    /// offline, a timeout or an unreachable server (all worth trying again);
    /// for a `LocalizedError`, its description. Anything else gets a generic
    /// title and message, never Swift's "The operation couldn't be
    /// completed (… error 3)".
    public init(_ error: Error) {
        if let described = error as? ForgeDescribedError {
            self = described.failure
        } else if let network = Self.network(error) {
            self = network
        } else if let localized = error as? LocalizedError, let description = localized.errorDescription {
            self.init(title: "Something Went Wrong", message: description)
        } else {
            self.init(title: "Something Went Wrong", message: "Try again. If it keeps happening, close the app and open it again.")
        }
    }

    /// Words for the URLSession errors a person can do something about.
    private static func network(_ error: Error) -> ForgeFailure? {
        guard let error = error as? URLError else { return nil }
        switch error.code {
        case .notConnectedToInternet, .dataNotAllowed, .networkConnectionLost, .internationalRoamingOff:
            return ForgeFailure(title: "You're Offline", message: "Try again when you're connected.", isRetryable: true)
        case .timedOut:
            return ForgeFailure(title: "No Response", message: "The server took too long to answer. Try again in a moment.", isRetryable: true)
        case .cannotFindHost, .cannotConnectToHost, .dnsLookupFailed:
            return ForgeFailure(title: "Can't Reach the Server", message: "Check your connection, then try again.", isRetryable: true)
        default:
            return nil
        }
    }

    /// Whether `error` means the work was stopped on purpose (a Swift task
    /// cancelled, a sheet closed). Stopped work is not a failure: AppKit
    /// shows nothing for it.
    public static func isCancellation(_ error: Error) -> Bool {
        if error is CancellationError { return true }
        if let described = error as? ForgeDescribedError { return described.isCancellation }
        return false
    }
}

/// An app error that knows how to describe itself. Apps conform their Rust
/// error type once, and every alert, failed load and failed submission uses
/// the same words:
///
///     extension LibraryError: ForgeDescribedError {
///         public var failure: ForgeFailure {
///             switch self {
///             case .Offline: ForgeFailure(title: "You're Offline", message: "…", isRetryable: true)
///             …
///             }
///         }
///         public var isCancellation: Bool { if case .Cancelled = self { true } else { false } }
///     }
public protocol ForgeDescribedError: Error {
    var failure: ForgeFailure { get }
    /// True when the error only says the work was stopped. Defaults to false.
    var isCancellation: Bool { get }
}

extension ForgeDescribedError {
    public var isCancellation: Bool { false }
}
