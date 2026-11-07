@preconcurrency import ForgeFFI
import Foundation
import Observation

/// The app's presentation state. It holds no reading rules: every change goes
/// to the Rust `Library` (rust/src/lib.rs), and everything shown is read back
/// from it, after a command or when a `LibraryEvent` arrives.
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

    public var searchText = "" { didSet { reload() } }
    public var statusFilter: ReadingStatus? { didSet { reload() } }
    public var sort: LibrarySort = .recentlyAdded { didSet { reload() } }

    @ObservationIgnored public private(set) var library: Library?
    @ObservationIgnored private let storageDirectory: String?
    @ObservationIgnored private var events: LibraryEvents?

    /// - Parameter storageDirectory: where Rust keeps the library. Defaults to
    ///   Application Support/PocketLibrary; tests pass a temporary folder.
    public init(storageDirectory: String? = nil) {
        self.storageDirectory = storageDirectory
    }

    // MARK: Lifecycle

    /// Opens the library once. Called from `.task`, so a view that appears
    /// again never opens a second instance.
    public func open() async {
        guard library == nil else { return }
        await openLibrary { try Library(storageDirectory: $0) }
    }

    /// After an unreadable library: keeps the old file as a backup and
    /// starts empty.
    public func startFresh() async {
        guard library == nil else { return }
        await openLibrary { try Library.openFresh(storageDirectory: $0) }
    }

    private func openLibrary(_ make: @escaping @Sendable (String) throws -> Library) async {
        phase = .opening
        do {
            let directory = try storageDirectory ?? Self.defaultDirectory()
            // Opening reads files: off the main actor.
            let library = try await Task.detached { try make(directory) }.value
            // Subscribe before the first read, so no change slips between.
            let events = try library.events()
            self.library = library
            self.events = events
            settings = library.settings()
            sort = settings.defaultSort
            reload()
            phase = .open
        } catch let error as LibraryError {
            if case .NewerLibrary = error {
                phase = .failed(message: ErrorText.message(error), canStartFresh: false)
            } else {
                phase = .failed(message: ErrorText.message(error), canStartFresh: true)
            }
        } catch {
            phase = .failed(message: error.localizedDescription, canStartFresh: false)
        }
    }

    /// Applies Rust's events until the stream ends (`close()`) or the
    /// calling task is cancelled. Run it from one `.task` on the root view.
    /// Cancelling a Swift task does not reach Rust by itself, so the
    /// cancellation handler ends the stream explicitly.
    public func listenForEvents() async {
        guard let events else { return }
        await withTaskCancellationHandler {
            while let event = await events.next() {
                switch event {
                case .changed, .eventsDropped:
                    reload()
                case .settingsChanged(let settings):
                    self.settings = settings
                }
            }
        } onCancel: {
            events.cancel()
        }
    }

    /// The app came back to the foreground: dates may have moved on ("this
    /// year", "last 7 days"), so read everything again. Nothing needs saving
    /// when going to the background: Rust saves every change before a
    /// command returns.
    public func becameActive() {
        reload()
    }

    /// Ends the event stream and closes the Rust library. For tests and
    /// teardown; the app keeps one library for its whole life.
    public func close() {
        events?.cancel()
        library?.shutdown()
        library = nil
        events = nil
    }

    // MARK: Reading

    /// Re-reads the current view of the library from Rust (memory only).
    public func reload() {
        guard let library else { return }
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

    public func searchCatalogue(_ query: String) async throws -> [CatalogueBook] {
        guard let library else { throw LibraryError.ShutDown }
        return try await library.searchCatalogue(query: query)
    }

    public func importFromCatalogue(_ key: String) async throws -> ImportedBook {
        try await run { try await $0.importFromCatalogue(key: key) }
    }

    /// Imports an EPUB chosen in the document picker. The picker's URL is
    /// security-scoped: access is held while Rust reads the file.
    public func importEpub(at url: URL) async throws -> Book {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        return try await run { try await $0.importEpub(path: url.path) }
    }

    public func updateSettings(_ settings: LibrarySettings) async throws {
        self.settings = try await run { try await $0.updateSettings(settings: settings) }
    }

    /// Writes an export file and returns its URL for the share sheet.
    public func exportLibrary() async throws -> URL {
        let path = try await run { try await $0.exportLibrary() }
        return URL(fileURLWithPath: path)
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

    private static func defaultDirectory() throws -> String {
        let base = try FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("PocketLibrary", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.path
    }
}
