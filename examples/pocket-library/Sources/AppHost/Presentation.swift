@preconcurrency import PocketLibraryFFI
import ForgeUI
import Foundation
import SwiftUI

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

    /// The status's ForgeUI tone: badges, swipe actions and progress bars
    /// take their colour from the theme through it.
    public var tone: ForgeTone {
        switch self {
        case .wantToRead: return .warning
        case .reading: return .info
        case .finished: return .success
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
