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
import uniffi.pocket_library_core.LibrarySort
import uniffi.pocket_library_core.ReadingStatus
import java.io.File
import org.junit.Assert.assertNull
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
    private val opener = LibraryOpener()

    @Before
    fun open() = runBlocking {
        env = ForgeEnvironment.temporary(root)
        host = newHost()
        model = LibraryModel(host, scope, opener)
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
        open = opener::open,
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

    private suspend fun relaunch() {
        host.report(AppPhase.BACKGROUND)
        host.close()
        host = newHost()
        model = LibraryModel(host, scope, opener)
        host.start()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
    }

    /**
     * The reader: the sample EPUB imports as a readable book, its chapters open, and the place
     * the reader saves is where it opens again after a relaunch. (The sample is too short to
     * move the page, so its status stays as it was.)
     */
    @Test
    fun aSavedReadingPlaceSurvivesARelaunch() = runBlocking {
        model.addSampleBook(File(root, "Sample Book.epub"))
        val book = model.all.value.single()
        assertEquals("The Lighthouse Keeper", book.title)
        assertTrue(book.canRead)
        assertNull(book.readingPosition)
        assertFalse(File(root, "Sample Book.epub").exists())

        val contents = model.openReader(book.id)
        assertEquals(2, contents.chapters.size)
        assertEquals(0u, contents.start.chapter)
        val second = model.chapter(book.id, 1u)
        assertEquals("Morning", second.section.title)
        // The heading, then three paragraphs.
        assertEquals(4, second.section.blocks.size)
        assertNull(second.start)

        val saved = model.saveReadingPosition(book.id, chapter = 1u, block = 2)
        assertEquals(1u, saved.readingPosition?.chapter)

        relaunch()
        val reopened = model.openReader(book.id)
        assertEquals(1u, reopened.start.chapter)
        assertEquals(2u, model.chapter(book.id, 1u).start?.block)
        // The second sample is refused: an empty library only.
        model.addSampleBook(File(root, "Sample Book.epub"))
        model.reload()
        assertEquals(1, model.all.value.size)
    }

    /** Importing a file that isn't an EPUB is refused in the iPhone app's words. */
    @Test
    fun aFileThatIsntAnEpubIsRefused() = runBlocking {
        val file = File(root, "notes.epub").apply { writeText("not a book") }
        val error = runCatching { model.importEpub(file.path) }.exceptionOrNull()
        assertTrue("expected UnsupportedFile, got $error", error is LibraryException.UnsupportedFile)
        assertEquals("Can't Read That File", ForgeFailure.from(error!!).title)
        model.reload()
        assertTrue(model.all.value.isEmpty())
    }

    /** PL-1 and PL-2: a page on an unread book starts it, and books and progress survive a relaunch. */
    @Test
    fun pl1_pl2_progressStartsABookAndSurvivesARelaunch() = runBlocking {
        val book = model.add(LibraryModel.emptyInput().copy(title = "Dune", totalPages = 400u))
        assertEquals(ReadingStatus.WANT_TO_READ, book.status)

        val read = model.updateProgress(book.id, 100u)
        assertEquals(ReadingStatus.READING, read.status)
        assertEquals(0.25, read.progress, 0.0001)
        assertEquals("Page 100 of 400", LibraryModel.pageSummary(read))

        relaunch()
        val after = awaitBooks { it.isNotEmpty() }.single()
        assertEquals("Dune", after.title)
        assertEquals(100u, after.currentPage)
        assertEquals(ReadingStatus.READING, after.status)
    }

    /** PL-3: search, the status filter and the sort order come from Rust. */
    @Test
    fun pl3_searchFilterAndSortChangeWhichBooksShow() = runBlocking {
        val emma = model.add(input("Emma", "Jane Austen"))
        val dune = model.add(input("Dune", "Frank Herbert"))
        model.setStatus(dune.id, ReadingStatus.FINISHED)
        assertEquals(listOf(dune.id, emma.id), model.books.value.map { it.id })

        model.setSort(LibrarySort.TITLE)
        assertEquals(listOf("Dune", "Emma"), model.books.value.map { it.title })

        model.setSearchText("austen")
        assertEquals(listOf("Emma"), model.books.value.map { it.title })
        model.setSearchText("")

        model.setStatusFilter(ReadingStatus.FINISHED)
        assertEquals(listOf("Dune"), model.books.value.map { it.title })
        // A book's screen still finds a book the list doesn't show.
        assertEquals("Emma", model.book(emma.id)?.title)
        assertEquals(2, model.all.value.size)
    }

    /** PL-6: a change made elsewhere (straight to the core here) reaches the library. */
    @Test
    fun pl6_aChangeMadeElsewhereReachesTheLibrary() = runBlocking {
        val book = model.add(input("Emma", "Jane Austen"))
        model.setStatusFilter(ReadingStatus.FINISHED)
        assertTrue(model.books.value.isEmpty())

        host.core!!.library.setStatus(book.id, ReadingStatus.FINISHED)
        assertEquals(listOf(book.id), awaitBooks { it.isNotEmpty() }.map { it.id })
    }

    /** PL-5: an unreadable library can be started fresh, and the unreadable file is kept. */
    @Test
    fun pl5_anUnreadableLibraryCanBeStartedFreshAndIsKept() = runBlocking {
        host.close()
        val data = File(env.dataDirectory)
        File(data, "library.json").writeText("not a library")

        host = newHost()
        model = LibraryModel(host, scope, opener)
        host.start()
        val failed = host.status.value
        assertTrue("expected a failure, got $failed", failed is ForgeHost.Status.Failed)
        assertTrue(LibraryModel.canStartFresh(failed))
        assertTrue(withTimeout(5_000) { model.canStartFresh.first { it } })

        model.startFresh().join()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
        assertFalse(opener.fresh)
        assertTrue(model.books.value.isEmpty())
        val kept = data.listFiles()!!.filter { it.name.startsWith("library.unreadable-") }
        assertEquals(1, kept.size)
        assertEquals("not a library", kept.single().readText())
    }

    @Test
    fun aLibraryFromANewerAppOrOpenElsewhereIsNeverStartedFresh() {
        assertFalse(LibraryModel.canStartFresh(ForgeHost.Status.Failed("x", LibraryException.InUse("held"))))
        assertFalse(LibraryModel.canStartFresh(ForgeHost.Status.Failed("x", LibraryException.NewerLibrary(9u, 1u))))
        assertFalse(LibraryModel.canStartFresh(ForgeHost.Status.Failed("x", IllegalStateException())))
        assertFalse(LibraryModel.canStartFresh(ForgeHost.Status.Ready))
    }

    /** The edit form starts from the book and saves the same book back. */
    @Test
    fun theEditFormRoundTripsABook() = runBlocking {
        val book = model.add(
            LibraryModel.emptyInput().copy(
                title = "Piranesi", author = "Susanna Clarke", totalPages = 272u, rating = 4u,
                notes = "The House", description = "A labyrinth", coverUrl = "https://example.com/c.jpg",
            ),
        )
        val text = org.forgemobile.pocketlibrary.ui.BookFormText.from(book)
        assertEquals("", text.currentPage)
        assertEquals(book.title, text.input().title)
        val edited = model.update(book.id, text.copy(author = "S. Clarke", rating = null).input())
        assertEquals("S. Clarke", edited.author)
        assertNull(edited.rating)
        assertEquals("The House", edited.notes)
        assertEquals("https://example.com/c.jpg", edited.coverUrl)
        assertEquals("Piranesi by S. Clarke is on my reading list.", LibraryModel.shareText(edited))
    }
}
