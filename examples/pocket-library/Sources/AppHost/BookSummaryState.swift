@preconcurrency import PocketLibraryFFI
import Foundation
import Observation

/// The whole-book summary ("read the whole book"): the saved one, or one
/// being written. Rust runs the swarm (Library.summariseBook): a reader and
/// writer per chapter, a cast keeper after each, then the overview. This
/// holds what the sheet shows while it does.
@Observable
@MainActor
public final class BookSummaryState {
    public enum Phase: Equatable {
        case loading, empty, working, ready, failed
    }

    public private(set) var phase: Phase = .loading
    public private(set) var synopsis: BookSynopsis?
    public private(set) var error: Error?
    /// Where the run is: the chapter, and what is happening inside it.
    public private(set) var chapter = 0
    public private(set) var chapters = 0
    public private(set) var chapterTitle = ""
    public private(set) var stage: SummaryStage = .opening
    public private(set) var writingOverview = false
    /// When the current run started, for the elapsed time.
    public private(set) var startedAt: Date?
    public let info: SummaryModelInfo

    private let library: LibraryModel
    private let bookID: UInt64
    private let model: SummaryModel
    @ObservationIgnored private var task: Task<Void, Never>?

    public init(library: LibraryModel, bookID: UInt64, model: SummaryModel) {
        self.library = library
        self.bookID = bookID
        self.model = model
        info = model.describe()
    }

    public func load() {
        synopsis = library.bookSynopsis(bookID)
        phase = synopsis == nil ? .empty : .ready
    }

    public func start() {
        guard phase != .working else { return }
        let previous = synopsis
        phase = .working
        error = nil
        chapter = 0
        chapters = 0
        writingOverview = false
        stage = .opening
        startedAt = Date()
        let relay = SummaryProgressRelay { [weak self] stage in self?.apply(stage) }
        task = Task {
            do {
                synopsis = try await library.summariseBook(bookID, model: model, progress: relay)
                phase = .ready
            } catch let error as LibraryError where error.isCancellation {
                synopsis = previous
                phase = previous == nil ? .empty : .ready
            } catch {
                synopsis = previous
                self.error = error
                phase = .failed
            }
            startedAt = nil
        }
    }

    public func stop() {
        task?.cancel()
    }

    private func apply(_ stage: SummaryStage) {
        switch stage {
        case .chapter(let chapter, let chapters, let title, _):
            self.chapter = Int(chapter)
            self.chapters = Int(chapters)
            chapterTitle = title
        case .overview:
            writingOverview = true
        default:
            break
        }
        self.stage = stage
    }

    /// "Chapter 12 of 50: The Ides" or "Writing the overview".
    public var headline: String {
        if writingOverview { return "Writing the overview" }
        if chapters == 0 { return "Opening the book…" }
        return "Chapter \(chapter) of \(chapters): \(chapterTitle)"
    }

    /// What is happening inside the chapter.
    public var detail: String {
        switch stage {
        case .opening: return "Opening the book…"
        case .chapter(_, _, _, let saved): return saved ? "Using the saved summary" : "Starting…"
        case .readingPart(let part, let parts):
            return parts == 1 ? "Reader: listing what happens" : "Reader: part \(part) of \(parts)"
        case .combining: return writingOverview ? "Summarising stretches of the book" : "Combining notes"
        case .writing: return writingOverview ? "Writing" : "Writer: summarising the chapter"
        case .revising: return "Editor: putting it in its own words"
        case .updatingCast: return "Cast keeper: updating who's who"
        case .overview: return "Bringing the chapters together"
        }
    }

    /// The text being written now: a chapter summary, or the overview.
    public var draft: String {
        if case .writing(let draft) = stage { return draft }
        return ""
    }

    /// How far through the book, 0–1. Chapters are most of the work.
    public var fractionDone: Double {
        if writingOverview { return 0.95 }
        guard chapters > 0 else { return 0 }
        return 0.92 * Double(max(chapter - 1, 0)) / Double(chapters)
    }
}
