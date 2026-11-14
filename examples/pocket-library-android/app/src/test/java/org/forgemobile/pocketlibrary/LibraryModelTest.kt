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
import org.junit.Assert.assertEquals
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

    private suspend fun awaitBooks(predicate: (List<uniffi.pocket_library_core.Book>) -> Boolean) =
        withTimeout(5_000) { model.books.first(predicate) }

    @Test
    fun addsOpensAndDeletesABook() = runBlocking {
        assertTrue(model.books.value.isEmpty())

        val book = model.add("The Left Hand of Darkness", "Ursula K. Le Guin")
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
        val error = runCatching { model.add("   ", "Someone") }.exceptionOrNull()
        assertTrue("expected a LibraryException, got $error", error is LibraryException)
        assertTrue(LibraryModel.describe(error!!).isNotBlank())
    }

    @Test
    fun booksAreSavedAcrossLaunches() = runBlocking {
        model.add("Piranesi", "Susanna Clarke")
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
