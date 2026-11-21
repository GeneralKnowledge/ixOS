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
import kotlinx.coroutines.withContext
import android.util.Log
import java.io.File
import org.forgemobile.pocketlibrary.host.ForgeCore
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.kit.ForgeLoad
import uniffi.forge_app.AppPhase
import uniffi.forge_app.Cancellation
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvents
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.CatalogueBook
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

    suspend fun updateSettings(settings: LibrarySettings) {
        _settings.value = run { it.updateSettings(settings) }
    }

    /** Switches online search on or off (Settings' Online Catalogue). */
    suspend fun setCatalogueEnabled(enabled: Boolean) = updateSettings(_settings.value.copy(catalogueEnabled = enabled))

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

    /**
     * Saves where the reader is: a block of the chapter loaded last, which Rust keeps as an
     * anchor. Rust moves the book's page to match, so the library and the book's screen update.
     */
    suspend fun saveReadingPosition(id: ULong, chapter: UInt, block: Int): Book = run {
        it.saveReadingPosition(id, chapter, ReaderLocation(block.coerceAtLeast(0).toUInt(), 0.0))
    }

    /**
     * Saves the reader's place as the reader closes, in the model's scope so leaving doesn't
     * cancel it. A failure is only logged: the reader is gone, and its last regular save stands.
     */
    fun saveReadingPositionOnLeave(id: ULong, chapter: UInt, block: Int): Job = scope.launch {
        try {
            saveReadingPosition(id, chapter, block)
        } catch (e: Exception) {
            Log.w(TAG, "reading position not saved on leaving the reader: ${e.javaClass.simpleName}")
        }
    }

    /**
     * Test data only (iOS `-PocketLibrarySampleBook`): once the library is open, and only if it is
     * empty, imports a short sample EPUB so a UI test has a book to read. [file] is a scratch
     * path for the EPUB, deleted afterwards.
     */
    suspend fun addSampleBook(file: File) {
        host.status.first { it == ForgeHost.Status.Ready || it is ForgeHost.Status.Failed }
        val core = library ?: return
        if (core.books(ALL_BOOKS).isNotEmpty()) return
        try {
            withContext(RUST_CALLS) { writeSampleEpub(file.path) }
            importEpub(file.path)
        } catch (e: Exception) {
            Log.w(TAG, "sample book not added: ${e.javaClass.simpleName}")
        } finally {
            file.delete()
        }
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
