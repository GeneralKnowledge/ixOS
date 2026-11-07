@preconcurrency import ForgeFFI
import Foundation
import SwiftUI

/// User-facing text for Rust's typed errors. `reason` fields are
/// diagnostics, shown as detail.
public enum ErrorText {
    public static func title(_ error: Error) -> String {
        guard let error = error as? LibraryError else { return "Something went wrong" }
        switch error {
        case .InvalidInput: return "Check the details"
        case .NotFound: return "That book is gone"
        case .AlreadyInLibrary: return "Already in your library"
        case .Storage, .NewerLibrary: return "Couldn't save"
        case .CatalogueDisabled: return "Online search is off"
        case .Offline: return "You're offline"
        case .Timeout: return "The catalogue didn't answer"
        case .CatalogueUnavailable: return "The catalogue is unavailable"
        case .BadResponse: return "Unexpected answer from the catalogue"
        case .Certificate: return "Secure connection failed"
        case .Network: return "Network problem"
        case .UnsupportedFile: return "Can't read that file"
        case .ShutDown: return "The library is closed"
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

extension ReadingStatus: @retroactive CaseIterable, @retroactive Identifiable {
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

extension SortOrder: @retroactive CaseIterable, @retroactive Identifiable {
    public static var allCases: [SortOrder] { [.recentlyAdded, .title, .author, .progress] }
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

extension Book: @retroactive Identifiable {}
extension CatalogueBook: @retroactive Identifiable {
    public var id: String { key }
}

extension Book {
    var displayAuthor: String { author.isEmpty ? "Unknown author" : author }

    var pageSummary: String {
        switch (status, totalPages) {
        case (.finished, _): return "Finished"
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
