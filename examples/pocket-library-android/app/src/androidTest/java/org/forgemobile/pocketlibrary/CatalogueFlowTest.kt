package org.forgemobile.pocketlibrary

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.platform.io.PlatformTestStorageRegistry
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHttpTransport
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.pocket_library_core.HttpRequest
import uniffi.pocket_library_core.HttpResponse
import uniffi.pocket_library_core.HttpStreamBody
import uniffi.pocket_library_core.HttpStreamHead
import uniffi.pocket_library_core.HttpTransport
import uniffi.pocket_library_core.HttpTransportException
import uniffi.pocket_library_core.installNativeHttp

/**
 * PL-7 and PL-8 on an emulator. The catalogue's requests go to a stand-in for Android's HTTP
 * transport, installed as the app installs the real one: no network, fixed answers, and the
 * test sees every request Rust sends and every one it stops.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class CatalogueFlowTest {
    /** Answers searches for "the hobbit"; holds any other search open until Rust stops it. */
    private class StandIn : HttpTransport {
        val requests = CopyOnWriteArrayList<HttpRequest>()
        val cancelled = CopyOnWriteArrayList<ULong>()
        val held = CountDownLatch(1)
        val stopped = CountDownLatch(1)

        override fun send(request: HttpRequest): HttpResponse {
            requests.add(request)
            val url = request.url
            return when {
                "/search.json" in url && "hobbit" in url -> ok(
                    """{"docs": [{"key": "/works/OL1W", "title": "The Hobbit", "author_name": ["J.R.R. Tolkien"],
                       "first_publish_year": 1937, "number_of_pages_median": 310}]}""",
                )
                "/search.json" in url -> {
                    held.countDown()
                    stopped.await(20, TimeUnit.SECONDS)
                    throw HttpTransportException.Cancelled()
                }
                "/works/OL1W" in url -> ok("""{"description": "A hobbit goes on an adventure."}""")
                else -> HttpResponse(404u, emptyList(), ByteArray(0))
            }
        }

        override fun openStream(request: HttpRequest, body: HttpStreamBody): HttpStreamHead =
            throw HttpTransportException.Transport("no streams here")

        override fun cancel(id: ULong) {
            cancelled.add(id)
            stopped.countDown()
        }

        private fun ok(json: String) = HttpResponse(200u, emptyList(), json.toByteArray())
    }

    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null
    private val transport = StandIn()

    @Before
    fun launch() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, true)
        scenario = ActivityScenario.launch(intent)
        compose.waitUntilAtLeastOneExists(hasTestTag("library.empty"), 20_000)
        installNativeHttp(transport)
    }

    @After
    fun finish() {
        transport.stopped.countDown()
        scenario?.close()
        ForgeHttpTransport.install()
    }

    private fun openCatalogue() {
        compose.onNodeWithTag("library.empty.add").performClick()
        compose.onNodeWithTag("library.add.catalogue").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("catalogue.search"), 5_000)
    }

    private fun search(text: String) {
        compose.onNodeWithTag("catalogue.search").performTextReplacement(text)
        compose.onNodeWithTag("catalogue.search").performImeAction()
    }

    /** Finding a book and adding it opens it, with the catalogue's details. */
    @Test
    fun aBookFoundInTheCatalogueIsAddedAndOpened() {
        openCatalogue()
        search("the hobbit")
        compose.waitUntilExactlyOneExists(hasTestTag("catalogue.results"), 10_000)
        saveScreenshot("catalogue-results")
        compose.onNodeWithTag("catalogue.add").performClick()

        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.title") and hasText("The Hobbit"), 10_000)
        // Further down the book's screen, so it exists rather than being on screen.
        compose.onNode(hasText("A hobbit goes on an adventure.")).assertExists()
        assertTrue(transport.requests.all { it.url.startsWith("https://openlibrary.org/") })
    }

    /** PL-8: leaving a catalogue search stops it: Rust tells the transport to stop the request. */
    @Test
    fun pl8_leavingASearchStopsIt() {
        openCatalogue()
        search("tolkien")
        if (!transport.held.await(20, TimeUnit.SECONDS)) throw AssertionError("the search didn't go out: ${screen()}")
        compose.onNodeWithTag("catalogue.searching").assertIsDisplayed()
        compose.onNodeWithTag("catalogue.cancel").performClick()

        compose.waitUntilExactlyOneExists(hasTestTag("library.empty"), 5_000)
        assertTrue("Rust stopped the request", transport.stopped.await(5, TimeUnit.SECONDS))
        val search = transport.requests.single { "/search.json" in it.url }
        assertEquals(listOf(search.id), transport.cancelled.toList())
    }

    /** PL-7: with online search off, the catalogue can't be opened, it says why, and nothing is sent. */
    @Test
    fun pl7_switchedOffTheCatalogueSaysWhyAndSendsNothing() {
        compose.onNodeWithTag("library.settings").performClick()
        compose.onNodeWithTag("settings.onlineCatalogue").performClick()
        compose.waitUntil(5_000) {
            compose.onNodeWithTag("settings.onlineCatalogue").fetchSemanticsNode()
                .config.getOrNull(SemanticsProperties.ToggleableState)?.name == "Off"
        }
        compose.onNodeWithTag("settings.back").performClick()

        compose.onNodeWithTag("library.empty.add").performClick()
        compose.onNodeWithTag("library.add.catalogue").assertIsNotEnabled()
        compose.onNodeWithTag("library.add.catalogue.off", useUnmergedTree = true)
            .assertTextEquals("Online search is off in Settings")
        assertTrue(transport.requests.isEmpty())
    }

    /** What the catalogue screen shows and what was sent, for a failure message. */
    private fun screen(): String {
        val shown = listOf("catalogue.idle", "catalogue.searching", "catalogue.results", "catalogue.error", "catalogue.noResults")
            .filter { compose.onAllNodes(hasTestTag(it)).fetchSemanticsNodes().isNotEmpty() }
        val error = compose.onAllNodes(hasTestTag("catalogue.error")).fetchSemanticsNodes().firstOrNull()
            ?.config?.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }
        val field = compose.onAllNodes(hasTestTag("catalogue.search")).fetchSemanticsNodes().firstOrNull()
            ?.config?.getOrNull(SemanticsProperties.EditableText)?.text
        return "showing $shown, error $error, field \"$field\", requests ${transport.requests.map { it.url }}"
    }

    private fun saveScreenshot(name: String) {
        compose.waitForIdle()
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        PlatformTestStorageRegistry.getInstance().openOutputFile("screenshots/$name.png").use {
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }
}
