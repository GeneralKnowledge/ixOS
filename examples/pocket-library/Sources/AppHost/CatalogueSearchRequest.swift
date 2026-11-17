import Foundation

/// One catalogue search as the Find a Book screen runs it: the words, and
/// which attempt this is. The screen restarts its search whenever this value
/// changes, so Try Again (the same words, the next attempt) runs the search
/// that failed again, and a new search replaces one still running.
public struct CatalogueSearchRequest: Equatable {
    public let text: String
    public let attempt: Int

    /// The search for what was typed, or nil when there is nothing to find.
    public static func submitted(_ query: String, after previous: CatalogueSearchRequest?) -> CatalogueSearchRequest? {
        let text = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return nil }
        return CatalogueSearchRequest(text: text, attempt: (previous?.attempt ?? 0) + 1)
    }

    /// Try Again: the same words, as a new attempt.
    public var retried: CatalogueSearchRequest {
        CatalogueSearchRequest(text: text, attempt: attempt + 1)
    }
}
