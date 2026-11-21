// Pocket Library's screen state on Android: plain Kotlin and coroutines (no Android UI), so the
// JVM unit tests drive it against the real Rust core. The Rust core owns the books and the
// rules; this only asks for them and re-reads when the host says they changed, like the
// SwiftUI app's LibraryModel.
package org.forgemobile.pocketlibrary

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import android.util.Log
import java.io.File
import org.forgemobile.pocketlibrary.host.ForgeCore
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.kit.ForgeLoad
import uniffi.forge_app.AppPhase
import uniffi.forge_app.Cancellation
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvents
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.CatalogueBook
import uniffi.pocket_library_core.CsvImportSummary
import uniffi.pocket_library_core.StorageInfo
import uniffi.pocket_library_core.ImportedBook
import uniffi.pocket_library_core.LibrarySettings
import uniffi.pocket_library_core.BookInput
import uniffi.pocket_library_core.LibraryException
import uniffi.pocket_library_core.LibraryQuery
import uniffi.pocket_library_core.LibrarySort
import uniffi.pocket_library_core.PocketLibrary
import uniffi.pocket_library_core.ReadingStatus
import uniffi.pocket_library_core.ReaderChapter
import uniffi.pocket_library_core.ReaderContents
import uniffi.pocket_library_core.ReaderLocation
import uniffi.pocket_library_core.writeSampleEpub

/** Pocket Library's Rust core, as the host sees it. */
class LibraryCore(val library: PocketLibrary) : ForgeCore {
    override fun setPhase(phase: AppPhase) = library.setPhase(phase)
    override fun events(): RuntimeEvents? = library.events()
    override fun revision(): ULong = library.revision()
    override fun backgroundWorkPending(): Boolean = library.backgroundWorkPending()
    override fun backgroundTimeExpired() = library.backgroundTimeExpired()
    override fun launchReport(): LaunchReport = library.launchReport()
    override fun shutdown() = library.shutdown()
    override fun close() = library.destroy()
}

class LibraryModel(
    private val host: ForgeHost<LibraryCore>,
    private val scope: CoroutineScope,
    private val opener: LibraryOpener,
) {
    init {
        // Alerts, failed loads and failed saves use ErrorText's words (ForgeAppKit).
        ForgeFailure.describer = ErrorText
    }

    private val _all = MutableStateFlow<List<Book>>(emptyList())
    private val _books = MutableStateFlow<List<Book>>(emptyList())
    private val _searchText = MutableStateFlow("")
    private val _statusFilter = MutableStateFlow<ReadingStatus?>(null)
    private val _sort = MutableStateFlow(LibrarySort.RECENTLY_ADDED)
    private val _settings = MutableStateFlow(DEFAULT_SETTINGS)

    /** Every book, newest first: what a book's screen reads, whatever the list shows. */
    val all: StateFlow<List<Book>> = _all.asStateFlow()

    /** The books the library list shows: Rust applies the search, filter and sort. */
    val books: StateFlow<List<Book>> = _books.asStateFlow()

    val searchText: StateFlow<String> = _searchText.asStateFlow()

    /** The status the list is limited to, or null for every book. */
    val statusFilter: StateFlow<ReadingStatus?> = _statusFilter.asStateFlow()

    val sort: StateFlow<LibrarySort> = _sort.asStateFlow()

    /** The library's settings, as Rust last saved them. */
    val settings: StateFlow<LibrarySettings> = _settings.asStateFlow()

    /** Opening the library: loading, open, or why it couldn't open (with Try Again). */
    val opening: StateFlow<ForgeLoad<Unit>> = host.status
        .map(::openingState)
        .stateIn(scope, SharingStarted.Eagerly, openingState(host.status.value))

    /**
     * Whether the library that failed to open may be set aside for an empty one. Never for a
     * library from a newer version of the app, or one that is open elsewhere.
     */
    val canStartFresh: StateFlow<Boolean> = host.status
        .map(::canStartFresh)
        .stateIn(scope, SharingStarted.Eagerly, canStartFresh(host.status.value))

    init {
        // Re-read whenever Rust's state may have changed: commits from any screen, and the
        // core opening.
        scope.launch { host.changes.collect { reload() } }
        // A newly opened library starts in the order the person chose in Settings.
        scope.launch {
            var wasReady = false
            host.status.collect { status ->
                val ready = status == ForgeHost.Status.Ready
                if (ready && !wasReady) {
                    library?.let {
                        _settings.value = it.settings()
                        _sort.value = _settings.value.defaultSort
                        reload()
                    }
                }
                wasReady = ready
            }
        }
    }

    private val library: PocketLibrary?
        get() = host.core?.library

    /**
     * Whether the last run ended while an import or export was still finishing in the
     * background, so it may not have happened.
     */
    val lastRunWasCutShort: Boolean
        get() = host.launch?.unfinishedBackgroundWork == true

    /** Opens the library, or tries again after a failure. Does nothing once open. */
    fun open() {
        scope.launch { host.start() }
    }

    /**
     * After an unreadable library: keeps the old file as a backup and opens an empty library.
     * Does nothing while a library is open.
     */
    fun startFresh(): Job = scope.launch {
        // In the model's scope, not the screen's: the failure screen that asked leaves as soon
        // as opening starts, and its scope would cancel the open.
        if (host.core != null) return@launch
        opener.fresh = true
        host.start()
    }

    fun reload() {
        val core = library
        _all.value = core?.books(ALL_BOOKS) ?: emptyList()
        _books.value = core?.books(query()) ?: emptyList()
    }

    fun query() = LibraryQuery(text = _searchText.value, status = _statusFilter.value, sort = _sort.value)

    fun setSearchText(text: String) {
        _searchText.value = text
        reload()
    }

    fun setStatusFilter(status: ReadingStatus?) {
        _statusFilter.value = status
        reload()
    }

    /** A new launch starts on the whole library, unsearched (the sort comes from Settings). */
    fun resetView() {
        _searchText.value = ""
        _statusFilter.value = null
        reload()
    }

    fun setSort(sort: LibrarySort) {
        _sort.value = sort
        reload()
    }

    fun book(id: ULong): Book? = _all.value.firstOrNull { it.id == id }

    // Commands. Each throws the Rust LibraryException, so the screen that started it shows the
    // problem where the person is: field errors under the fields, anything else as an alert.

    private suspend fun <T> run(command: suspend (PocketLibrary) -> T): T {
        val core = library ?: throw LibraryException.ShutDown()
        val result = withContext(RUST_CALLS) { command(core) }
        // The event updates the screen too; reading now avoids a flicker.
        reload()
        return result
    }

    /** Adds a book. The core validates it: invalid fields come back as `InvalidFields`. */
    suspend fun add(input: BookInput): Book = run { it.addBook(input) }

    /** Saves the edit form. The core validates it as it does a new book. */
    suspend fun update(id: ULong, input: BookInput): Book = run { it.updateBook(id, input) }

    /** Records the page reached. Rust starts an unread book and finishes one at its last page. */
    suspend fun updateProgress(id: ULong, page: UInt): Book = run { it.updateProgress(id, page) }

    suspend fun setStatus(id: ULong, status: ReadingStatus): Book = run { it.setStatus(id, status) }

    suspend fun delete(id: ULong) {
        run { it.deleteBook(id) }
    }

    /**
     * Changes the saved settings. Rust replaces the whole record, so changes run one at a time,
     * each starting from the settings the one before saved; two at once can't undo each other.
     */
    private suspend fun changeSettings(change: (LibrarySettings) -> LibrarySettings) {
        changingSettings.withLock {
            val settings = change(_settings.value)
            _settings.value = run { it.updateSettings(settings) }
        }
    }

    /** Settings' default order: saved in Rust, and the library list follows it straight away. */
    suspend fun setDefaultSort(sort: LibrarySort) {
        changeSettings { it.copy(defaultSort = sort) }
        setSort(sort)
    }

    /** Switches online search on or off (Settings' Online Catalogue). */
    suspend fun setCatalogueEnabled(enabled: Boolean) = changeSettings { it.copy(catalogueEnabled = enabled) }

    /**
     * Searches the online catalogue. Cancelling the calling coroutine (leaving the screen, or a
     * newer search) stops the Rust search and its request. With online search off, Rust refuses
     * before any request: `CatalogueDisabled`.
     */
    suspend fun searchCatalogue(query: String): List<CatalogueBook> {
        val core = library ?: throw LibraryException.ShutDown()
        return withRustCancellation { cancellation ->
            withContext(RUST_CALLS) { core.searchCatalogue(query, cancellation) }
        }
    }

    /** Adds a catalogue book with its cover and description. Cancelling stops it; nothing is added. */
    suspend fun importFromCatalogue(key: String): ImportedBook = run { core ->
        withRustCancellation { core.importFromCatalogue(key, it) }
    }

    /**
     * Runs [block] in the model's scope: work the person started that finishes even if the
     * screen that started it goes (a backup, a restore), as a Swift `Task` does.
     */
    fun finish(block: suspend () -> Unit): Job = scope.launch { block() }

    private val _dataBusy = MutableStateFlow(false)
    private val _dataResult = MutableStateFlow<ForgeAlert<Nothing>?>(null)

    /** A backup, restore, spreadsheet or erase is running (one at a time). */
    val dataBusy: StateFlow<Boolean> = _dataBusy.asStateFlow()

    /** What the last of those came to, until the person dismisses it. */
    val dataResult: StateFlow<ForgeAlert<Nothing>?> = _dataResult.asStateFlow()

    fun clearDataResult() {
        _dataResult.value = null
    }

    /**
     * Runs one data operation in the model's scope, so Settings can close and open again while
     * it runs and still show it running, then its result. [block] returns the alert to show (or
     * null); [failure] turns an error into one. False, doing nothing, while another runs.
     */
    fun runDataWork(
        failure: (Exception) -> ForgeAlert<Nothing>?,
        block: suspend () -> ForgeAlert<Nothing>?,
    ): Boolean {
        if (_dataBusy.value) return false
        _dataBusy.value = true
        scope.launch {
            try {
                _dataResult.value = block()
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                _dataResult.value = failure(e)
            } finally {
                _dataBusy.value = false
            }
        }
        return true
    }

    // Your data: backups, spreadsheets, erasing. Each is finishing work in Rust, so it
    // completes when the person leaves the app (the host holds background time).

    /** How much the library holds, for Settings; null while it isn't open. It counts files, so off the main thread. */
    suspend fun storageInfo(): StorageInfo? = withContext(Dispatchers.IO) { library?.storageInfo() }

    /**
     * Writes a backup of the whole library (books, notes, history, settings, covers and EPUBs)
     * and returns its path, for the screen to copy where the person chose.
     */
    suspend fun backupLibrary(): String = run { it.backupLibrary() }

    /**
     * Replaces the library with the backup at [path]. A file that isn't a backup, or a damaged
     * one, is refused and the library is left as it was. Returns how many books it has.
     */
    suspend fun restoreLibrary(path: String): UInt {
        // The backup brings its own settings; a settings change waits, then starts from them.
        val (books, sort) = changingSettings.withLock {
            val books = run { it.restoreLibrary(path) }
            library?.let { _settings.value = it.settings() }
            books to _settings.value.defaultSort
        }
        setSort(sort)
        return books
    }

    /** Writes every book as a spreadsheet (CSV) and returns its path. */
    suspend fun exportBooksCsv(): String = run { it.exportBooksCsv() }

    /** Adds the books in a CSV file; rows that can't be read are skipped and listed by line. */
    suspend fun importBooksCsv(path: String): CsvImportSummary = run { it.importBooksCsv(path) }

    /** Deletes every book, the history and saved covers and EPUBs. Settings stay. */
    suspend fun eraseLibrary() = run { it.eraseLibrary() }

    // Reading EPUBs

    /**
     * Imports an EPUB. [path] is a file Rust can read: the screen copies the document the person
     * picked, and may delete its copy afterwards (Rust keeps its own). A book already in the
     * library without an EPUB gets this one, keeping its progress.
     */
    suspend fun importEpub(path: String): Book = run { it.importEpub(path) }

    /** Opens a book's saved EPUB: its chapters and where to start. */
    suspend fun openReader(id: ULong): ReaderContents {
        val core = library ?: throw LibraryException.ShutDown()
        return withContext(RUST_CALLS) { core.openReader(id) }
    }

    /** One chapter's text, from the book [openReader] opened, with its saved place the first time. */
    suspend fun chapter(id: ULong, index: UInt): ReaderChapter {
        val core = library ?: throw LibraryException.ShutDown()
        return withContext(RUST_CALLS) { core.readerChapter(id, index) }
    }

    /** Reading-place saves, one at a time and in the order they were asked for. */
    private val savingPlace = Mutex()
    private val changingSettings = Mutex()

    /**
     * Saves where the reader is: a block of the chapter loaded last, which Rust keeps as an
     * anchor. Rust moves the book's page to match, so the library and the book's screen update.
     */
    suspend fun saveReadingPosition(id: ULong, chapter: UInt, block: Int): Book = savingPlace.withLock {
        run { it.saveReadingPosition(id, chapter, ReaderLocation(block.coerceAtLeast(0).toUInt(), 0.0)) }
    }

    /**
     * Saves the reader's place in the model's scope, so a reader that closes, or moves to another
     * chapter, doesn't cancel the save. The returned job ends when the save has; [onFailure]
     * gets the error in the model's scope, the main thread (the failure is logged either way).
     */
    fun saveReadingPositionLater(
        id: ULong,
        chapter: UInt,
        block: Int,
        onFailure: ((Exception) -> Unit)? = null,
    ): Job = scope.launch {
        try {
            saveReadingPosition(id, chapter, block)
        } catch (e: Exception) {
            if (e is CancellationException) throw e
            Log.w(TAG, "reading position not saved: ${e.javaClass.simpleName}")
            onFailure?.invoke(e)
        }
    }

    /**
     * Test data only (iOS `-PocketLibrarySampleBook`): once the library is open, and only if it is
     * empty, imports a short sample EPUB so a UI test has a book to read. [file] is a scratch
     * path for the EPUB, deleted afterwards.
     */
    suspend fun addSampleBook(file: File) {
        try {
            if (writeSampleBook(file)) importEpub(file.path)
        } catch (e: Exception) {
            Log.w(TAG, "sample book not added: ${e.javaClass.simpleName}")
        } finally {
            file.delete()
        }
    }

    /**
     * Test data only: once the library is open, and only if it is empty, writes the sample EPUB
     * to [file] for an import. False when there is nothing to import.
     */
    suspend fun writeSampleBook(file: File): Boolean {
        host.status.first { it == ForgeHost.Status.Ready || it is ForgeHost.Status.Failed }
        val core = library ?: return false
        if (core.books(ALL_BOOKS).isNotEmpty()) return false
        withContext(RUST_CALLS) { writeSampleEpub(file.path) }
        return true
    }

    companion object {
        private const val TAG = "PocketLibrary"

        /**
         * Where the model awaits Rust's async calls. UniFFI resumes the Kotlin coroutine from
         * Rust's waker; on a dispatcher that resumes in place (Compose's, in UI tests) the next
         * poll then runs inside that callback and the app crashes in JNA. A pool dispatcher
         * always posts the resume.
         */
        val RUST_CALLS = Dispatchers.Default

        /** Until the library opens: as a new library starts. */
        val DEFAULT_SETTINGS = LibrarySettings(defaultSort = LibrarySort.RECENTLY_ADDED, catalogueEnabled = true)

        /**
         * Runs [body] with a Rust `Cancellation` that is cancelled when the calling coroutine is
         * (ForgeKit's `withRustCancellation`). Rust then stops the work, and tells the host to
         * stop its request.
         */
        suspend fun <T> withRustCancellation(body: suspend (Cancellation) -> T): T {
            val cancellation = Cancellation()
            try {
                // Already cancelled: Rust refuses before it starts.
                if (!currentCoroutineContext().isActive) cancellation.cancel()
                return body(cancellation)
            } catch (e: CancellationException) {
                cancellation.cancel()
                throw e
            }
        }

        val ALL_BOOKS = LibraryQuery(text = "", status = null, sort = LibrarySort.RECENTLY_ADDED)

        /** A new book's empty form, as the iPhone app starts it. */
        fun emptyInput(): BookInput = BookInput(
            title = "",
            author = "",
            description = "",
            status = ReadingStatus.WANT_TO_READ,
            currentPage = 0u,
            totalPages = null,
            rating = null,
            notes = "",
            coverUrl = null,
        )

        fun openingState(status: ForgeHost.Status): ForgeLoad<Unit> = when (status) {
            ForgeHost.Status.Ready -> ForgeLoad.Loaded(Unit)
            is ForgeHost.Status.Failed -> {
                val error = status.error
                val message = if (error is LibraryException) ErrorText.message(error) else status.message
                ForgeLoad.Failed(ForgeFailure("Can't Open Library", message, isRetryable = true))
            }
            ForgeHost.Status.Loading, ForgeHost.Status.Closed -> ForgeLoad.Loading
        }

        fun canStartFresh(status: ForgeHost.Status): Boolean {
            val error = (status as? ForgeHost.Status.Failed)?.error as? LibraryException ?: return false
            return error !is LibraryException.NewerLibrary && error !is LibraryException.InUse
        }

        fun sortLabel(sort: LibrarySort): String = when (sort) {
            LibrarySort.RECENTLY_ADDED -> "Recently Added"
            LibrarySort.TITLE -> "Title"
            LibrarySort.AUTHOR -> "Author"
            LibrarySort.PROGRESS -> "Progress"
        }

        /** What Share sends, as the iPhone app words it. */
        fun shareText(book: Book): String {
            var text = book.title
            if (book.author.isNotEmpty()) text += " by ${book.author}"
            text = when (book.status) {
                ReadingStatus.FINISHED -> "I finished $text"
                ReadingStatus.READING -> {
                    val total = book.totalPages
                    "I'm reading $text" +
                        if (total != null) " (${Math.round(book.progress * 100)}% of $total pages)" else ""
                }
                ReadingStatus.WANT_TO_READ -> "$text is on my reading list"
            }
            book.rating?.let { text += " — " + "★".repeat(it.toInt()) }
            return "$text."
        }

        /** The row's reading summary, as the iPhone app words it. */
        fun pageSummary(book: Book): String {
            val total = book.totalPages
            return when {
                book.status == ReadingStatus.FINISHED -> "Finished"
                book.status == ReadingStatus.WANT_TO_READ && total != null -> "$total pages"
                total != null -> "Page ${book.currentPage} of $total"
                book.status == ReadingStatus.READING -> if (book.currentPage > 0u) "Page ${book.currentPage}" else "Started"
                else -> "Not started"
            }
        }

        fun statusLabel(status: ReadingStatus): String = when (status) {
            ReadingStatus.WANT_TO_READ -> "Want to Read"
            ReadingStatus.READING -> "Reading"
            ReadingStatus.FINISHED -> "Finished"
        }
    }
}
