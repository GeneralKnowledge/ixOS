// Pocket Library's screen state on Android: plain Kotlin and coroutines (no Android UI), so the
// JVM unit tests drive it against the real Rust core. The Rust core owns the books and the
// rules; this only asks for them and re-reads when the host says they changed, like the
// SwiftUI app's LibraryModel.
package org.forgemobile.pocketlibrary

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.host.ForgeCore
import org.forgemobile.pocketlibrary.host.ForgeHost
import uniffi.forge_app.AppPhase
import uniffi.forge_app.ErrorKind
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvents
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.BookInput
import uniffi.pocket_library_core.PocketLibrary
import uniffi.pocket_library_core.LibraryException
import uniffi.pocket_library_core.LibraryQuery
import uniffi.pocket_library_core.LibrarySort
import uniffi.pocket_library_core.ReadingStatus
import uniffi.pocket_library_core.libraryErrorKind

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
    private val _books = MutableStateFlow<List<Book>>(emptyList())

    /** Every book, newest first. */
    val books: StateFlow<List<Book>> = _books.asStateFlow()

    val status: StateFlow<ForgeHost.Status> get() = host.status

    init {
        // Re-read whenever Rust's state may have changed: commits from any screen, and the
        // core opening.
        scope.launch { host.changes.collect { reload() } }
    }

    private val library: PocketLibrary?
        get() = host.core?.library

    fun reload() {
        _books.value = library?.books(ALL_BOOKS) ?: emptyList()
    }

    fun book(id: ULong): Book? = _books.value.firstOrNull { it.id == id }

    /**
     * Adds a book from the form. The core validates it; an invalid form comes back as
     * [LibraryException], whose text [describe] gives.
     */
    suspend fun add(title: String, author: String): Book {
        val core = library ?: throw IllegalStateException("The library isn't open yet.")
        return core.addBook(
            BookInput(
                title = title,
                author = author,
                description = "",
                status = ReadingStatus.WANT_TO_READ,
                currentPage = 0u,
                totalPages = null,
                rating = null,
                notes = "",
                coverUrl = null,
            ),
        )
    }

    suspend fun delete(id: ULong) {
        val core = library ?: throw IllegalStateException("The library isn't open yet.")
        core.deleteBook(id)
    }

    companion object {
        val ALL_BOOKS = LibraryQuery(text = "", status = null, sort = LibrarySort.RECENTLY_ADDED)

        /** Text for an error, by the framework's error kind (ForgeErrors.describe in Swift). */
        fun describe(error: Throwable): String {
            if (error is LibraryException.InvalidFields) {
                return error.fields.joinToString("\n") { it.message }
            }
            if (error is LibraryException.InvalidInput) return error.reason
            if (error !is LibraryException) return error.message ?: "Something went wrong."
            return when (libraryErrorKind(error)) {
                ErrorKind.INVALID -> "Check what you entered."
                ErrorKind.NOT_FOUND -> "That book no longer exists."
                ErrorKind.UNAVAILABLE -> "Not available right now. Try again later."
                ErrorKind.INTERRUPTED -> "Stopped when the app went to the background. Try again."
                ErrorKind.CANCELLED -> "Cancelled."
                ErrorKind.NEEDS_USER -> "This needs your permission or sign-in."
                ErrorKind.CORRUPT -> "The saved library couldn't be read."
                ErrorKind.NEWER_DATA -> "This library was saved by a newer version of the app. Update the app to open it."
                ErrorKind.SHUT_DOWN -> "The app is closing."
                ErrorKind.INTERNAL -> "Something went wrong."
            }
        }

        fun statusLabel(status: ReadingStatus): String = when (status) {
            ReadingStatus.WANT_TO_READ -> "Want to Read"
            ReadingStatus.READING -> "Reading"
            ReadingStatus.FINISHED -> "Finished"
        }
    }
}
