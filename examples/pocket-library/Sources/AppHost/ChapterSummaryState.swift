@preconcurrency import PocketLibraryFFI
import Foundation
import Observation

/// The reader's chapter summary sheet: the saved summary of a chapter, or
/// one being written. Rust plans and saves the summary (rust/src/summary.rs);
/// this holds what the sheet shows while it does.
@Observable
@MainActor
public final class ChapterSummaryState {
    public enum Phase: Equatable {
        case loading
        /// No summary yet.
        case empty
        case working
        case ready
        case failed
    }

    public private(set) var phase: Phase = .loading
    /// What the summary is doing, while `phase` is `.working`.
    public private(set) var stage: SummaryStage = .opening
    public private(set) var synopsis: ChapterSynopsis?
    public private(set) var error: Error?
    /// Whether the model can run, and why not.
    public let info: SummaryModelInfo

    private let library: LibraryModel
    private let bookID: UInt64
    private let chapter: UInt32
    private let model: SummaryModel
    @ObservationIgnored private var task: Task<Void, Never>?

    public init(library: LibraryModel, bookID: UInt64, chapter: UInt32, model: SummaryModel) {
        self.library = library
        self.bookID = bookID
        self.chapter = chapter
        self.model = model
        info = model.describe()
    }

    /// Reads the saved summary, if any.
    public func load() async {
        do {
            synopsis = try await library.chapterSynopsis(bookID, index: chapter)
            phase = synopsis == nil ? .empty : .ready
        } catch {
            self.error = error
            phase = .failed
        }
    }

    /// Writes a summary (again, if there is one). The old one stays on
    /// screen and saved until the new one is done.
    public func summarise() {
        guard phase != .working else { return }
        let previous = synopsis
        phase = .working
        stage = .opening
        error = nil
        let relay = SummaryProgressRelay { [weak self] stage in self?.stage = stage }
        task = Task {
            do {
                synopsis = try await library.summariseChapter(bookID, index: chapter, model: model, progress: relay)
                phase = .ready
            } catch let error as LibraryError where error.isCancellation {
                synopsis = previous
                phase = previous == nil ? .empty : .ready
            } catch {
                synopsis = previous
                self.error = error
                phase = .failed
            }
        }
    }

    /// Stops a summary being written; nothing is saved.
    public func stop() {
        task?.cancel()
    }

    /// "Reading part 2 of 5…"
    public var stageText: String {
        switch stage {
        case .opening: return "Opening the chapter…"
        case .readingPart(let part, let parts): return "Reading part \(part) of \(parts)…"
        case .combining: return "Bringing the parts together…"
        case .writing: return "Writing the summary…"
        case .revising: return "Putting it in its own words…"
        case .chapter(let chapter, let chapters, let title, _): return "Chapter \(chapter) of \(chapters): \(title)"
        case .updatingCast: return "Updating who's who…"
        case .overview: return "Writing the overview…"
        }
    }

    /// How far along, 0–1, for a progress bar: reading the parts is most
    /// of the work.
    public var fractionDone: Double {
        switch stage {
        case .opening: return 0
        case .readingPart(let part, let parts): return 0.85 * Double(part - 1) / Double(max(parts, 1))
        case .combining: return 0.85
        case .writing: return 0.9
        case .revising: return 0.95
        case .chapter, .updatingCast, .overview: return 0
        }
    }

    /// The summary so far, while it is written.
    public var draft: String {
        if case .writing(let draft) = stage { return draft }
        return ""
    }
}

/// Passes Rust's progress, which arrives on a Rust thread, to the main
/// actor in order.
final class SummaryProgressRelay: SummaryProgress, Sendable {
    private let deliver: @MainActor @Sendable (SummaryStage) -> Void

    init(_ deliver: @escaping @MainActor @Sendable (SummaryStage) -> Void) {
        self.deliver = deliver
    }

    func changed(stage: SummaryStage) {
        DispatchQueue.main.async {
            MainActor.assumeIsolated { self.deliver(stage) }
        }
    }
}
