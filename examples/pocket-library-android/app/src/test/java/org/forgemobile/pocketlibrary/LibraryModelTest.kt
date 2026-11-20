package org.forgemobile.pocketlibrary

import java.nio.file.Files
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.junit.After
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.kit.ForgeLoad
import org.forgemobile.pocketlibrary.ui.BookFormText
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.forge_app.AppPhase
import uniffi.pocket_library_core.PocketLibrary
import uniffi.pocket_library_core.LibraryException

/**
 * The Android host and model against Pocket Library's real Rust core (the host build of
 * rust/, `cargo build --manifest-path rust/Cargo.toml`), the same core the iPhone app runs.
 */
class LibraryModelTest {
    private val root = Files.createTempDirectory("pocket-library-android").toFile()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private lateinit var env: ForgeEnvironment
    private lateinit var host: ForgeHost<LibraryCore>
    private lateinit var model: LibraryModel

    @Before
    fun open() = runBlocking {
        env = ForgeEnvironment.temporary(root)
        host = newHost()
        model = LibraryModel(host, scope)
        host.start()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
    }

    @After
    fun close() {
        host.close()
        scope.cancel()
        root.deleteRecursively()
    }

    private fun newHost() = ForgeHost(
        scope = scope,
        environment = { env },
        open = { LibraryCore(PocketLibrary(it.dataDirectory)) },
    )

    private fun input(title: String, author: String) = LibraryModel.emptyInput().copy(title = title, author = author)

    private suspend fun awaitBooks(predicate: (List<uniffi.pocket_library_core.Book>) -> Boolean) =
        withTimeout(5_000) { model.books.first(predicate) }

    @Test
    fun addsOpensAndDeletesABook() = runBlocking {
        assertTrue(model.books.value.isEmpty())

        val book = model.add(input("The Left Hand of Darkness", "Ursula K. Le Guin"))
        // The runtime announces the commit; the model re-reads.
        val books = awaitBooks { it.isNotEmpty() }
        assertEquals(listOf(book.id), books.map { it.id })
        assertEquals("Ursula K. Le Guin", model.book(book.id)?.author)

        model.delete(book.id)
        awaitBooks { it.isEmpty() }
        Unit
    }

    @Test
    fun theCoreRejectsABookWithoutATitle() = runBlocking {
        val error = runCatching { model.add(input("   ", "Someone")) }.exceptionOrNull()
        assertTrue("expected InvalidFields, got $error", error is LibraryException.InvalidFields)
        val errors = ErrorText.fieldErrors(error as LibraryException.InvalidFields)
        assertEquals("Enter a title.", errors["title"])
        assertEquals("Check the Details", ForgeFailure.from(error).title)
    }

    /** PL-4: invalid details are refused with a plain-language message, and change nothing. */
    @Test
    fun invalidDetailsAreRefusedByFieldAndChangeNothing() = runBlocking {
        val text = BookFormText(title = "Short Book", currentPage = "50", totalPages = "10")
        assertTrue(text.localErrors().isEmpty)
        val error = runCatching { model.add(text.input()) }.exceptionOrNull()
        assertTrue("expected InvalidFields, got $error", error is LibraryException.InvalidFields)
        val errors = ErrorText.fieldErrors(error as LibraryException.InvalidFields)
        assertEquals(listOf("current_page"), errors.fields)
        assertEquals("The current page is past the last page (10).", errors["current_page"])
        model.reload()
        assertTrue(model.books.value.isEmpty())
    }

    /** Text that isn't a number never reaches Rust; the form says so under the field. */
    @Test
    fun pagesThatArentNumbersAreCaughtInTheForm() {
        val errors = BookFormText(title = "A", currentPage = "ten", totalPages = "-3").localErrors()
        assertEquals(listOf("current_page", "total_pages"), errors.fields)
        assertEquals("Enter a whole number.", errors["total_pages"])
        assertTrue(BookFormText(title = "A", currentPage = " 12 ", totalPages = "").localErrors().isEmpty)
        assertFalse(BookFormText(title = "  ").canSave)
    }

    @Test
    fun aFailedOpenOffersTryAgain() {
        val state = LibraryModel.openingState(ForgeHost.Status.Failed("disk on fire", LibraryException.InUse("held")))
        val failure = (state as ForgeLoad.Failed).failure
        assertEquals("Can't Open Library", failure.title)
        assertEquals("Another copy of Pocket Library has this library open. Close it and try again.", failure.message)
        assertTrue(failure.isRetryable)
        assertEquals(ForgeLoad.Loaded(Unit), LibraryModel.openingState(ForgeHost.Status.Ready))
    }

    @Test
    fun booksAreSavedAcrossLaunches() = runBlocking {
        model.add(input("Piranesi", "Susanna Clarke"))
        awaitBooks { it.size == 1 }
        host.report(AppPhase.BACKGROUND)
        host.close()

        host = newHost()
        model = LibraryModel(host, scope)
        host.start()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
        assertEquals(listOf("Piranesi"), awaitBooks { it.isNotEmpty() }.map { it.title })
    }
}
