@preconcurrency import ForgeApp
@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import Foundation

/// User-facing text for Rust's typed errors. `reason` fields are
/// diagnostics, shown as detail.
public enum ErrorText {
    public static func title(_ error: Error) -> String {
        guard let error = error as? LibraryError else { return "Something Went Wrong" }
        switch error {
        case .InvalidInput, .InvalidFields: return "Check the Details"
        case .NotFound: return "Book Not Found"
        case .AlreadyInLibrary: return "Already in Your Library"
        case .Storage, .NewerLibrary: return "Couldn't Save"
        case .CatalogueDisabled: return "Online Search Is Off"
        case .Offline: return "You're Offline"
        case .Timeout: return "Catalogue Didn't Respond"
        case .CatalogueUnavailable: return "Catalogue Unavailable"
        case .BadResponse: return "Unexpected Catalogue Response"
        case .Certificate: return "Secure Connection Failed"
        case .Network: return "Network Problem"
        case .UnsupportedFile: return "Can't Read That File"
        case .NotReadable: return "Can't Open This Book"
        case .Cancelled: return "Search Stopped"
        case .Interrupted: return "Stopped"
        case .ShutDown: return "Library Closed"
        case .InUse: return "Library Already Open"
        case .SummaryUnavailable: return "Summaries Unavailable"
        case .SummaryRefused: return "Couldn't Summarise"
        case .SummaryFailed: return "Summary Failed"
        }
    }

    public static func message(_ error: Error) -> String {
        guard let error = error as? LibraryError else { return error.localizedDescription }
        switch error {
        case .InvalidInput(let reason): return String(reason.prefix(1)).uppercased() + String(reason.dropFirst()) + "."
        case .InvalidFields(let fields): return fields.map(\.message).joined(separator: "\n")
        case .NotFound: return "It may have been deleted on another screen."
        case .AlreadyInLibrary: return "This book is already in your library."
        case .Storage(let reason): return "Your library was not changed. \(reason)"
        case .NewerLibrary:
            return "Your library was saved by a newer version of Pocket Library. Update the app to open it; nothing has been changed."
        case .CatalogueDisabled: return "Turn on online search in Settings, or add the book by hand."
        case .Offline: return "Your library still works offline. Try searching again when you're connected, or add the book by hand."
        case .Timeout: return "Try again in a moment, or add the book by hand."
        case .CatalogueUnavailable(let reason): return "Open Library is having trouble (\(reason)). Your library is unaffected."
        case .BadResponse(let reason): return "Open Library sent something Pocket Library couldn't read (\(reason))."
        case .Certificate(let reason): return "The server's certificate was rejected: \(reason)"
        case .Network(let reason): return reason
        case .UnsupportedFile(let reason): return "Pocket Library imports EPUB books. \(reason)"
        case .NotReadable(let reason): return String(reason.prefix(1)).uppercased() + String(reason.dropFirst()) + "."
        case .Cancelled: return "Nothing was changed."
        case .Interrupted: return "Pocket Library went to the background before this finished. Nothing was changed; try again."
        case .ShutDown: return "Restart the app to keep going."
        case .InUse: return "Another copy of Pocket Library has this library open. Close it and try again."
        case .SummaryUnavailable(let reason): return reason
        case .SummaryRefused: return "Apple Intelligence declined to summarise this chapter."
        case .SummaryFailed(let reason): return "Apple Intelligence couldn't finish the summary (\(reason)). Try again."
        }
    }

    /// Whether trying the same thing again might work.
    public static func isRetryable(_ error: Error) -> Bool {
        guard let error = error as? LibraryError else { return false }
        switch error {
        case .Offline, .Timeout, .CatalogueUnavailable, .Network, .Interrupted, .SummaryFailed: return true
        default: return false
        }
    }
}

/// Every alert, failed load and failed save in the app (ForgeAppKit) uses
/// ErrorText's words. A cancelled request is not a failure: nothing is shown.
extension LibraryError: ForgeDescribedError {
    public var failure: ForgeFailure {
        ForgeFailure(title: ErrorText.title(self), message: ErrorText.message(self), isRetryable: ErrorText.isRetryable(self))
    }

    public var isCancellation: Bool {
        if case .Cancelled = self { return true }
        return false
    }
}

extension ForgeFieldErrors {
    /// The form's errors from Rust's validation (`forge_app::Validator`):
    /// field names are `BookInput`'s, in Rust's spelling.
    init(_ fields: [FieldError]) {
        self.init(fields.map { ($0.field, $0.message) })
    }
}
