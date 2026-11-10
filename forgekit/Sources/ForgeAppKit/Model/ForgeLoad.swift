// Loading something a screen shows: not started, loading, loaded, or
// failed. Foundation only.

import Foundation

/// The state of something a screen loads (search results, a chapter, a
/// forecast), as one value instead of a value, a busy flag and an error
/// that can disagree. `ForgeLoadView` draws it.
///
///     public private(set) var results: ForgeLoad<[CatalogueBook]> = .idle
///
///     func search(_ text: String) async {
///         results = .loading
///         results = await .capture { try await library.searchCatalogue(query: text) }
///     }
public enum ForgeLoad<Value> {
    /// Nothing asked for yet.
    case idle
    case loading
    case loaded(Value)
    case failed(ForgeFailure)

    /// Runs `work` and returns its outcome: `.loaded` with the value,
    /// `.failed` with the error described, or `.idle` when the work was
    /// cancelled (a newer search replaced it, the screen closed).
    /// It runs on the caller's actor (the main actor, from a view model).
    public static func capture(
        isolation: isolated (any Actor)? = #isolation,
        _ work: () async throws -> Value
    ) async -> ForgeLoad {
        do {
            return .loaded(try await work())
        } catch {
            return ForgeFailure.isCancellation(error) ? .idle : .failed(ForgeFailure(error))
        }
    }

    public var value: Value? {
        if case .loaded(let value) = self { value } else { nil }
    }

    public var failure: ForgeFailure? {
        if case .failed(let failure) = self { failure } else { nil }
    }

    public var isLoading: Bool {
        if case .loading = self { true } else { false }
    }

    /// The same state with the loaded value transformed.
    public func map<T>(_ transform: (Value) -> T) -> ForgeLoad<T> {
        switch self {
        case .idle: .idle
        case .loading: .loading
        case .loaded(let value): .loaded(transform(value))
        case .failed(let failure): .failed(failure)
        }
    }
}

extension ForgeLoad: Equatable where Value: Equatable {}
extension ForgeLoad: Sendable where Value: Sendable {}
