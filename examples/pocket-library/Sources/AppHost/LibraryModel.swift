@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeKit
import Foundation
import Observation

// The Rust library runs on forge-app's runtime (`export_runtime!`), so
// ForgeKit can host it: one core per process, lifecycle, background time.
extension Library: ForgeCore {}

extension LibraryError: ForgeKindedError {
    public var forgeKind: ErrorKind { libraryErrorKind(error: self) }
}

/// What the next open of the library uses, handed to the host's opener,
/// which runs off the main actor. It keeps the opener's typed error, which
/// the host reduces to text, so the model can tell whether starting fresh
/// is safe.
private final class OpenRequest: @unchecked Sendable {
    private let lock = NSLock()
    private var environment: ForgeEnvironment?
    private var fresh = false
    private var error: LibraryError?

    func prepare(_ environment: ForgeEnvironment, fresh: Bool) {
        lock.withLock {
            self.environment = environment
            self.fresh = fresh
            error = nil
        }
    }

    func currentEnvironment() throws -> ForgeEnvironment {
        guard let environment = lock.withLock({ environment }) else { throw LibraryError.ShutDown }
        return environment
    }

    func open(_ environment: ForgeEnvironment) throws -> Library {
        let fresh = lock.withLock { self.fresh }
        do {
            return fresh
                ? try Library.openFresh(storageDirectory: environment.dataDirectory)
                : try Library(storageDirectory: environment.dataDirectory)
        } catch let failure as LibraryError {
            lock.withLock { error = failure }
            throw failure
        }
    }

    func takeError() -> LibraryError? {
        lock.withLock {
            defer { error = nil }
            return error
        }
    }
}

/// The app's presentation state. It holds no reading rules: every change goes
/// to the Rust `Library` (rust/src/lib.rs), and everything shown is read back
/// from it, after a command or whenever the host's `changes` moves.
@Observable
@MainActor
public final class LibraryModel {
    public enum Phase: Equatable {
        case opening
        case open
        /// The library could not be opened. `canStartFresh` is false when the
        /// file is from a newer app version (it must never be set aside).
        case failed(message: String, canStartFresh: Bool)
    }

    public private(set) var phase: Phase = .opening
    /// The books for the current search, filter and order.
    public private(set) var books: [Book] = []
    public private(set) var totalBooks = 0
    public private(set) var stats: LibraryStats?
    public private(set) var history: [ActivityEntry] = []
    public private(set) var settings = LibrarySettings(defaultSort: .recentlyAdded, catalogueEnabled: true)

    // MARK: The Library screen's presentation (ForgeAppKit)

    /// Where the Library screen is going: one sheet or picker at a time.
    public enum LibraryDestination: Equatable {
        case addBook
        case catalogue
        case importEpub
    }

    /// What the Library screen's alerts can ask the model to do.
    public enum LibraryAction: Equatable {
        case delete(UInt64)
    }

    public var libraryDestination: LibraryDestination?
    /// The Library screen's alert: a delete confirmation or an error.
    public var libraryAlert: ForgeAlert<LibraryAction>?

    /// Asks before deleting; confirming sends `.delete` to `handle`.
    public func askToDelete(_ book: Book) {
        libraryAlert = .confirm(
            "Delete \u{201C}\(book.title)\u{201D}?",
            message: "Your notes and progress for this book are deleted. Its reading history stays.",
            confirm: "Delete Book",
            action: .delete(book.id),
            identifier: "library.delete",
            destructive: true
        )
    }

    public func handle(_ action: LibraryAction) async {
        switch action {
        case .delete(let id):
            do { try await delete(id) } catch { libraryAlert = .failure(error) }
        }
    }

    public var searchText = "" { didSet { reload() } }
    public var statusFilter: ReadingStatus? { didSet { reload() } }
    public var sort: LibrarySort = .recentlyAdded { didSet { reload() } }

    /// Owns the Rust library for the life of the process: it opens it off
    /// the main actor, reports the app's lifecycle to it, holds background
    /// time while imports and exports finish, and moves `changes` whenever
    /// Rust commits. Give it to `ForgeLifecycle.attach` in the App's init.
    @ObservationIgnored public let host: ForgeHost<Library>
    @ObservationIgnored private let request: OpenRequest
    @ObservationIgnored private let storageDirectory: String?

    /// The open Rust library, or nil before it opens and after `close()`.
    public var library: Library? { host.core }

    /// Whether the last run ended while an import or export was still
    /// finishing in the background, so it may not have happened.
    public var lastRunWasCutShort: Bool { host.launch?.unfinishedBackgroundWork == true }

    /// - Parameter storageDirectory: where Rust keeps the library. Defaults to
    ///   Application Support/PocketLibrary; tests pass a temporary folder.
    public init(storageDirectory: String? = nil) {
        let request = OpenRequest()
        self.request = request
        self.storageDirectory = storageDirectory
        host = ForgeHost(environment: { try request.currentEnvironment() }, open: { try request.open($0) })
    }

    // MARK: Lifecycle

    /// Opens the library once. Called from `.task`, so a view that appears
    /// again never opens a second instance.
    public func open() async {
        guard library == nil else { return }
        await openLibrary(fresh: false)
    }

    /// After an unreadable library: keeps the old file as a backup and
    /// starts empty.
    public func startFresh() async {
        guard library == nil else { return }
        await openLibrary(fresh: true)
    }

    private func openLibrary(fresh: Bool) async {
        phase = .opening
        do {
            let directory = try storageDirectory ?? Self.defaultDirectory()
            // The library keeps no caches of its own.
            request.prepare(ForgeEnvironment(dataDirectory: directory, cachesDirectory: directory), fresh: fresh)
        } catch {
            phase = .failed(message: error.localizedDescription, canStartFresh: false)
            return
        }
        // Opening reads files: the host does it off the main actor, then
        // subscribes to Rust's events before anything is read.
        await host.start()
        switch host.status {
        case .ready:
            reload()
            sort = settings.defaultSort
            phase = .open
        case .failed(let text):
            guard let error = request.takeError() else {
                phase = .failed(message: text, canStartFresh: false)
                return
            }
            switch error {
            // A newer library must never be set aside, and one open
            // elsewhere isn't broken.
            case .NewerLibrary, .InUse:
                phase = .failed(message: ErrorText.message(error), canStartFresh: false)
            default:
                phase = .failed(message: ErrorText.message(error), canStartFresh: true)
            }
        case .loading, .closed:
            break
        }
    }

    /// Shuts the Rust library down. For tests and teardown; the app keeps
    /// one library for its whole life. Every change is already saved.
    public func close() {
        host.close()
    }

    // MARK: Reading

    /// Re-reads the current view of the library from Rust (memory only).
    /// The root view calls it whenever the host's `changes` moves: after a
    /// command commits, and on the way back from the background, when
    /// dates may have moved on ("this year", "last 7 days").
    public func reload() {
        guard let library else { return }
        settings = library.settings()
        books = library.books(query: LibraryQuery(text: searchText, status: statusFilter, sort: sort))
        stats = library.stats(yearStartMs: Self.startOfYearMs())
        totalBooks = Int(stats?.total ?? 0)
        history = library.history(bookId: nil, limit: 300)
    }

    public func book(id: UInt64) -> Book? {
        try? library?.book(id: id)
    }

    /// Every book with `status`, most read first (for "Reading Now").
    public func allBooks(status: ReadingStatus) -> [Book] {
        library?.books(query: LibraryQuery(text: "", status: status, sort: .progress)) ?? []
    }

    public func history(for book: Book) -> [ActivityEntry] {
        library?.history(bookId: book.id, limit: 50) ?? []
    }

    // MARK: Commands
    //
    // Each throws the Rust `LibraryError`, so the screen that started it can
    // show the problem where the user is (an alert on a sheet, say).

    private func run<T: Sendable>(_ command: (Library) async throws -> T) async throws -> T {
        guard let library else { throw LibraryError.ShutDown }
        let result = try await command(library)
        // The event updates the screen too; reading now avoids a flicker.
        reload()
        return result
    }

    @discardableResult
    public func add(_ input: BookInput) async throws -> Book {
        try await run { try await $0.addBook(input: input) }
    }

    @discardableResult
    public func update(_ id: UInt64, _ input: BookInput) async throws -> Book {
        try await run { try await $0.updateBook(id: id, input: input) }
    }

    @discardableResult
    public func updateProgress(_ id: UInt64, page: UInt32) async throws -> Book {
        try await run { try await $0.updateProgress(id: id, page: page) }
    }

    @discardableResult
    public func setStatus(_ id: UInt64, _ status: ReadingStatus) async throws -> Book {
        try await run { try await $0.setStatus(id: id, status: status) }
    }

    public func delete(_ id: UInt64) async throws {
        try await run { try await $0.deleteBook(id: id) }
    }

    /// Cancelling the calling task stops the Rust request too; it then
    /// throws `LibraryError.Cancelled`.
    public func searchCatalogue(_ query: String) async throws -> [CatalogueBook] {
        guard let library else { throw LibraryError.ShutDown }
        return try await withRustCancellation { try await library.searchCatalogue(query: query, cancellation: $0) }
    }

    /// Cancelling the calling task stops the import, and nothing is added.
    /// If the app leaves the screen part way, the import keeps going while
    /// iOS gives the app time, and fails with `Interrupted` if it runs out.
    public func importFromCatalogue(_ key: String) async throws -> ImportedBook {
        try await run { library in
            try await withRustCancellation { try await library.importFromCatalogue(key: key, cancellation: $0) }
        }
    }

    /// Imports an EPUB chosen in the document picker. The picker's URL is
    /// security-scoped: access is held while Rust reads the file.
    public func importEpub(at url: URL) async throws -> Book {
        #if canImport(Darwin)
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        #endif
        return try await run { try await $0.importEpub(path: url.path) }
    }

    // MARK: Reading EPUBs

    /// Opens a book's saved EPUB: its chapters and where to start.
    public func openReader(_ id: UInt64) async throws -> ReaderContents {
        guard let library else { throw LibraryError.ShutDown }
        return try await library.openReader(id: id)
    }

    /// One chapter's text, from the book `openReader` opened.
    public func chapter(_ id: UInt64, index: UInt32) async throws -> ReaderChapter {
        guard let library else { throw LibraryError.ShutDown }
        return try await library.readerChapter(id: id, index: index)
    }

    /// Saves where the reader is. Rust moves the book's page to match, so
    /// the library, history and stats update too.
    @discardableResult
    public func saveReadingPosition(_ id: UInt64, chapter: UInt32, fraction: Double) async throws -> Book {
        try await run {
            try await $0.saveReadingPosition(id: id, position: ReadingPosition(chapter: chapter, fraction: fraction))
        }
    }

    // MARK: Chapter summaries

    /// The saved summary of the chapter that reader chapter `index` is part
    /// of, for the book `openReader` opened.
    public func chapterSynopsis(_ id: UInt64, index: UInt32) async throws -> ChapterSynopsis? {
        guard let library else { throw LibraryError.ShutDown }
        return try await library.chapterSynopsis(id: id, index: index)
    }

    /// Writes and saves a summary of that chapter with `model`. Cancelling
    /// the calling task stops it, and nothing is saved.
    public func summariseChapter(
        _ id: UInt64, index: UInt32, model: SummaryModel, progress: SummaryProgress?
    ) async throws -> ChapterSynopsis {
        try await run { library in
            try await withRustCancellation {
                try await library.summariseChapter(
                    id: id, index: index, model: model, progress: progress, cancellation: $0)
            }
        }
    }

    /// The saved whole-book summary, if any.
    public func bookSynopsis(_ id: UInt64) -> BookSynopsis? {
        library?.bookSynopsis(id: id)
    }

    /// Reads the whole book with `model` (minutes) and saves its summary.
    /// Chapter summaries are saved as it goes, so a stopped run resumes.
    public func summariseBook(
        _ id: UInt64, model: SummaryModel, progress: SummaryProgress?
    ) async throws -> BookSynopsis {
        try await run { library in
            try await withRustCancellation {
                try await library.summariseBook(id: id, model: model, progress: progress, cancellation: $0)
            }
        }
    }

    public func updateSettings(_ settings: LibrarySettings) async throws {
        self.settings = try await run { try await $0.updateSettings(settings: settings) }
    }

    /// Writes a backup of the whole library and returns its URL for the
    /// share sheet.
    public func backupLibrary() async throws -> URL {
        let path = try await run { try await $0.backupLibrary() }
        return URL(fileURLWithPath: path)
    }

    /// Replaces the library with a backup chosen in the document picker
    /// (security-scoped while Rust reads it); returns how many books it has.
    public func restoreLibrary(at url: URL) async throws -> UInt32 {
        #if canImport(Darwin)
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        #endif
        return try await run { try await $0.restoreLibrary(path: url.path) }
    }

    /// Writes the books as a spreadsheet (CSV) and returns its URL for the
    /// share sheet.
    public func exportBooksCSV() async throws -> URL {
        let path = try await run { try await $0.exportBooksCsv() }
        return URL(fileURLWithPath: path)
    }

    /// Adds the books in a spreadsheet chosen in the document picker. Rows
    /// Rust can't read are skipped and listed in the summary.
    public func importBooksCSV(at url: URL) async throws -> CsvImportSummary {
        #if canImport(Darwin)
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        #endif
        return try await run { try await $0.importBooksCsv(path: url.path) }
    }

    public func eraseLibrary() async throws {
        try await run { try await $0.eraseLibrary() }
    }

    public func storageInfo() -> StorageInfo? {
        library?.storageInfo()
    }

    // MARK: Private

    static func startOfYearMs(_ now: Date = Date()) -> Int64 {
        let start = Calendar.current.dateInterval(of: .year, for: now)?.start ?? now
        return Int64(start.timeIntervalSince1970 * 1000)
    }

    /// The launch argument that starts the app on throwaway test data
    /// (`[ui_test] test_data_args` in forge.toml): `forge ui explore` and UI
    /// tests pass it so they never touch the person's own library.
    public static let testDataArgument = "-ForgeUITestData"

    /// Whether this launch uses throwaway test data; it is emptied once per
    /// launch. ForgeKit's `-ForgeTemporaryData` does the same, so one
    /// argument starts every app on test data (Device Lab's Apps screen).
    static let usesTestData = ProcessInfo.processInfo.arguments.contains(testDataArgument)
        || ProcessInfo.processInfo.arguments.contains("-ForgeTemporaryData")
    private static var testDataCleared = false

    private static func defaultDirectory() throws -> String {
        let name = usesTestData ? "PocketLibrary-UITest" : "PocketLibrary"
        let base = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent(name, isDirectory: true)
        if usesTestData && !testDataCleared {
            testDataCleared = true
            try? FileManager.default.removeItem(at: base)
        }
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
