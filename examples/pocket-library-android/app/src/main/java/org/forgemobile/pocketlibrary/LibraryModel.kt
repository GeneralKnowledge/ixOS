// Pocket Library's screen state on Android: plain Kotlin and coroutines (no Android UI), so the
// JVM unit tests drive it against the real Rust core. The Rust core owns the books and the
// rules; this only asks for them and re-reads when the host says they changed, like the
// SwiftUI app's LibraryModel.
package org.forgemobile.pocketlibrary

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.forgemobile.pocketlibrary.host.ForgeCore
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.kit.ForgeLoad
import uniffi.forge_app.AppPhase
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvents
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.BookInput
import uniffi.pocket_library_core.LibraryException
import uniffi.pocket_library_core.LibraryQuery
import uniffi.pocket_library_core.LibrarySort
import uniffi.pocket_library_core.PocketLibrary
import uniffi.pocket_library_core.ReadingStatus

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
) {
    init {
        // Alerts, failed loads and failed saves use ErrorText's words (ForgeAppKit).
        ForgeFailure.describer = ErrorText
    }

    private val _books = MutableStateFlow<List<Book>>(emptyList())

    /** Every book, newest first. */
    val books: StateFlow<List<Book>> = _books.asStateFlow()

    /** Opening the library: loading, open, or why it couldn't open (with Try Again). */
    val opening: StateFlow<ForgeLoad<Unit>> = host.status
        .map(::openingState)
        .stateIn(scope, SharingStarted.Eagerly, openingState(host.status.value))

    init {
        // Re-read whenever Rust's state may have changed: commits from any screen, and the
        // core opening.
        scope.launch { host.changes.collect { reload() } }
    }

    private val library: PocketLibrary?
        get() = host.core?.library

    /** Opens the library, or tries again after a failure. Does nothing once open. */
    fun open() {
        scope.launch { host.start() }
    }

    fun reload() {
        _books.value = library?.books(ALL_BOOKS) ?: emptyList()
    }

    fun book(id: ULong): Book? = _books.value.firstOrNull { it.id == id }

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

    suspend fun delete(id: ULong) {
        run { it.deleteBook(id) }
    }

    companion object {
        /**
         * Where the model awaits Rust's async calls. UniFFI resumes the Kotlin coroutine from
         * Rust's waker; on a dispatcher that resumes in place (Compose's, in UI tests) the next
         * poll then runs inside that callback and the app crashes in JNA. A pool dispatcher
         * always posts the resume.
         */
        val RUST_CALLS = Dispatchers.Default

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

        fun statusLabel(status: ReadingStatus): String = when (status) {
            ReadingStatus.WANT_TO_READ -> "Want to Read"
            ReadingStatus.READING -> "Reading"
            ReadingStatus.FINISHED -> "Finished"
        }
    }
}
