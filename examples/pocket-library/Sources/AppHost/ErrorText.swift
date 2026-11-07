@preconcurrency import ForgeFFI
import Foundation

/// User-facing text for Rust's typed errors. `reason` fields are
/// diagnostics, shown as detail.
public enum ErrorText {
    public static func title(_ error: Error) -> String {
        guard let error = error as? LibraryError else { return "Something Went Wrong" }
        switch error {
        case .InvalidInput: return "Check the Details"
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
        case .ShutDown: return "Library Closed"
        }
    }

    public static func message(_ error: Error) -> String {
        guard let error = error as? LibraryError else { return error.localizedDescription }
        switch error {
        case .InvalidInput(let reason): return String(reason.prefix(1)).uppercased() + String(reason.dropFirst()) + "."
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
        case .ShutDown: return "Restart the app to keep going."
        }
    }

    /// Whether trying the same thing again might work.
    public static func isRetryable(_ error: Error) -> Bool {
        guard let error = error as? LibraryError else { return false }
        switch error {
        case .Offline, .Timeout, .CatalogueUnavailable, .Network: return true
        default: return false
        }
    }
}
