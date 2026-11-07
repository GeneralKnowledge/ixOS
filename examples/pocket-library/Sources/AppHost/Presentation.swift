@preconcurrency import ForgeFFI
import Foundation
import SwiftUI

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

/// An error a screen is showing.
struct PresentedError: Identifiable {
    let id = UUID()
    let title: String
    let message: String

    init(_ error: Error) {
        title = ErrorText.title(error)
        message = ErrorText.message(error)
    }
}

extension View {
    /// Shows `error` as an alert with an OK button.
    func errorAlert(_ error: Binding<PresentedError?>) -> some View {
        alert(
            error.wrappedValue?.title ?? "",
            isPresented: Binding(
                get: { error.wrappedValue != nil },
                set: { if !$0 { error.wrappedValue = nil } }
            ),
            presenting: error.wrappedValue
        ) { _ in
            Button("OK", role: .cancel) {}
        } message: { presented in
            Text(presented.message)
        }
    }
}

extension ReadingStatus: CaseIterable, Identifiable {
    public static var allCases: [ReadingStatus] { [.wantToRead, .reading, .finished] }
    public var id: Self { self }

    public var label: String {
        switch self {
        case .wantToRead: return "Want to Read"
        case .reading: return "Reading"
        case .finished: return "Finished"
        }
    }

    public var symbol: String {
        switch self {
        case .wantToRead: return "bookmark"
        case .reading: return "book"
        case .finished: return "checkmark.circle"
        }
    }

    public var tint: Color {
        switch self {
        case .wantToRead: return .orange
        case .reading: return .blue
        case .finished: return .green
        }
    }
}

extension LibrarySort: CaseIterable, Identifiable {
    public static var allCases: [LibrarySort] { [.recentlyAdded, .title, .author, .progress] }
    public var id: Self { self }

    public var label: String {
        switch self {
        case .recentlyAdded: return "Recently Added"
        case .title: return "Title"
        case .author: return "Author"
        case .progress: return "Progress"
        }
    }
}

extension Book: Identifiable {}
extension CatalogueBook: Identifiable {
    public var id: String { key }
}

extension Book {
    var displayAuthor: String { author.isEmpty ? "Unknown author" : author }

    var pageSummary: String {
        switch (status, totalPages) {
        case (.finished, _): return "Finished"
        case (.wantToRead, let total?): return "\(total) pages"
        case (_, let total?): return "Page \(currentPage) of \(total)"
        case (.reading, nil): return currentPage > 0 ? "Page \(currentPage)" : "Started"
        default: return "Not started"
        }
    }

    /// What the share sheet sends.
    var shareText: String {
        var text = "\(title)"
        if !author.isEmpty { text += " by \(author)" }
        switch status {
        case .finished: text = "I finished \(text)"
        case .reading:
            text = "I'm reading \(text)"
            if let totalPages {
                text += " (\(Int((progress * 100).rounded()))% of \(totalPages) pages)"
            }
        case .wantToRead: text = "\(text) is on my reading list"
        }
        if let rating { text += " — " + String(repeating: "★", count: Int(rating)) }
        return text + "."
    }
}

enum Dates {
    static func date(_ ms: Int64) -> Date { Date(timeIntervalSince1970: Double(ms) / 1000) }

    static func short(_ ms: Int64) -> String {
        date(ms).formatted(date: .abbreviated, time: .omitted)
    }

    static func relative(_ ms: Int64) -> String {
        date(ms).formatted(.relative(presentation: .named))
    }
}
