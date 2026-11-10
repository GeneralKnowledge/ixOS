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
    /// `ForgeDescribedError`, otherwise a generic title and the error's
    /// description.
    public init(_ error: Error) {
        if let described = error as? ForgeDescribedError {
            self = described.failure
        } else {
            self.init(title: "Something Went Wrong", message: error.localizedDescription)
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
