package org.forgemobile.pocketlibrary

import java.nio.file.Files
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.forgemobile.pocketlibrary.ui.CatalogueSearchRequest
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import uniffi.forge_app.AppPhase
import uniffi.pocket_library_core.HttpRequest
import uniffi.pocket_library_core.HttpResponse
import uniffi.pocket_library_core.HttpStreamBody
import uniffi.pocket_library_core.HttpStreamHead
import uniffi.pocket_library_core.HttpTransport
import uniffi.pocket_library_core.HttpTransportException
import uniffi.pocket_library_core.LibraryException
import uniffi.pocket_library_core.installNativeHttp

/**
 * The catalogue against Pocket Library's real Rust core, with a stand-in for the host's HTTP
 * transport installed the way the app installs `ForgeHttpTransport`: no network, and the test
 * sees every request Rust asks the host to send, and every cancel.
 */
class CatalogueTest {
    /** Answers from [respond]; records what Rust asked for. */
    private class StandIn(val respond: (HttpRequest) -> HttpResponse) : HttpTransport {
        val requests = CopyOnWriteArrayList<HttpRequest>()
        val cancelled = CopyOnWriteArrayList<ULong>()
        val sent = CountDownLatch(1)
        val stopped = CountDownLatch(1)

        override fun send(request: HttpRequest): HttpResponse {
            requests.add(request)
            sent.countDown()
            return respond(request)
        }

        override fun openStream(request: HttpRequest, body: HttpStreamBody): HttpStreamHead =
            throw HttpTransportException.Transport("no streams here")

        override fun cancel(id: ULong) {
            cancelled.add(id)
            stopped.countDown()
        }
    }

    private fun ok(json: String) = HttpResponse(200u, emptyList(), json.toByteArray())

    private val root = Files.createTempDirectory("pocket-library-catalogue").toFile()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private lateinit var env: ForgeEnvironment
    private lateinit var host: ForgeHost<LibraryCore>
    private lateinit var model: LibraryModel
    private val opener = LibraryOpener()

    @Before
    fun open() = runBlocking {
        env = ForgeEnvironment.temporary(root)
        start()
    }

    private suspend fun start() {
        host = ForgeHost(scope = scope, environment = { env }, open = opener::open)
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

    private fun install(transport: StandIn) = installNativeHttp(transport).let { transport }

    @Test
    fun searchesAndImportsThroughTheHostsTransport() = runBlocking {
        val transport = install(
            StandIn { request ->
                when {
                    "/search.json" in request.url -> ok(
                        """{"docs": [{"key": "/works/OL1W", "title": "The Hobbit", "author_name": ["J.R.R. Tolkien"],
                           "first_publish_year": 1937, "number_of_pages_median": 310}]}""",
                    )
                    "/works/OL1W" in request.url -> ok("""{"description": "A hobbit goes on an adventure."}""")
                    else -> HttpResponse(404u, emptyList(), ByteArray(0))
                }
            },
        )
        val results = model.searchCatalogue("the hobbit")
        assertEquals(listOf("The Hobbit"), results.map { it.title })
        val url = transport.requests.first().url
        assertTrue(url, url.startsWith("https://openlibrary.org/search.json?q=the") && "hobbit" in url)

        val imported = model.importFromCatalogue(results.single().key)
        assertEquals("The Hobbit", imported.book.title)
        assertEquals("A hobbit goes on an adventure.", imported.book.description)
        assertEquals(310u, imported.book.totalPages)
        assertEquals(listOf("The Hobbit"), model.books.value.map { it.title })

        // Searching again shows it is in the library already.
        assertEquals(imported.book.id, model.searchCatalogue("the hobbit").single().libraryBookId)
    }

    /** PL-7: with online search off, no catalogue request is made, and the error says why. */
    @Test
    fun pl7_switchedOffMakesNoRequestAndSaysWhy() = runBlocking {
        val transport = install(StandIn { ok("""{"docs": []}""") })
        model.setCatalogueEnabled(false)
        assertFalse(model.settings.value.catalogueEnabled)

        val error = runCatching { model.searchCatalogue("dune") }.exceptionOrNull()
        assertTrue("expected CatalogueDisabled, got $error", error is LibraryException.CatalogueDisabled)
        assertEquals("Online Search Is Off", ErrorText.title(error as LibraryException))
        assertEquals("Turn on online search in Settings, or add the book by hand.", ErrorText.message(error))
        assertTrue("no request goes out", transport.requests.isEmpty())

        // The setting is saved with the library.
        host.report(AppPhase.BACKGROUND)
        host.close()
        start()
        assertFalse(model.settings.value.catalogueEnabled)
    }

    /** PL-8: leaving a search (its coroutine is cancelled) stops Rust, which stops the request. */
    @Test
    fun pl8_leavingASearchStopsTheRequest() = runBlocking {
        lateinit var transport: StandIn
        transport = install(
            StandIn {
                // Holds the request open until Rust cancels it.
                transport.stopped.await(10, TimeUnit.SECONDS)
                throw HttpTransportException.Cancelled()
            },
        )
        var outcome: Throwable? = null
        val search = scope.launch {
            outcome = runCatching { model.searchCatalogue("tolkien") }.exceptionOrNull()
        }
        assertTrue("the request went out", transport.sent.await(5, TimeUnit.SECONDS))
        search.cancel()
        assertTrue("Rust asked the host to stop the request", transport.stopped.await(5, TimeUnit.SECONDS))
        assertEquals(listOf(transport.requests.single().id), transport.cancelled.toList())
        search.join()
        assertTrue("a stopped search isn't a failure: $outcome", org.forgemobile.pocketlibrary.kit.ForgeFailure.isCancellation(outcome!!))
    }

    @Test
    fun tryAgainAndTheSameWordsAgainAreNewSearches() {
        assertNull(CatalogueSearchRequest.submitted("   ", after = null))
        val first = CatalogueSearchRequest.submitted("  dune ", after = null)!!
        assertEquals("dune", first.text)
        assertNotEquals(first, first.retried)
        assertNotEquals(first.retried, CatalogueSearchRequest.submitted("dune", after = first.retried))
    }
}
