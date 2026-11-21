package org.forgemobile.pocketlibrary

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.platform.io.PlatformTestStorageRegistry
import java.util.concurrent.CopyOnWriteArrayList
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.reader.ReadAloudSpeaker
import org.forgemobile.pocketlibrary.reader.Speaker
import org.forgemobile.pocketlibrary.reader.SystemSpeaker
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The reader and Read Aloud on an emulator (PL-9), on the sample EPUB a throwaway library starts
 * with (iOS: -PocketLibrarySampleBook). Read Aloud speaks through a stand-in that records what
 * it is given, installed as the app makes its speaker: the test checks that what would be said
 * is exactly the text on the screen, whatever voices the emulator has (it may have none).
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class ReaderFlowTest {
    /** Records each utterance, and says it to the end only when the test asks. */
    private class StandIn : Speaker {
        override var listener: Speaker.Listener? = null
        val said = CopyOnWriteArrayList<String>()

        @Volatile
        var lastId = ""

        override fun speak(text: String, id: String, speed: Double) {
            lastId = id
            said += text
            // The voice reaches the first word.
            listener?.onRange(id, 0, text.indexOf(' ').takeIf { it > 0 } ?: text.length)
        }

        override fun stop() = Unit

        override fun shutdown() {
            listener = null
        }
    }

    private val isLibraryRow = SemanticsMatcher("is a library.book row") { node ->
        node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("library.book.") == true
    }

    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null
    private val speaker = StandIn()

    @Before
    fun launch() {
        ReadAloudSpeaker.make = { speaker }
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, true)
            .putExtra(PocketLibraryApplication.SAMPLE_BOOK_EXTRA, true)
        scenario = ActivityScenario.launch(intent)
        compose.waitUntilAtLeastOneExists(hasText(SAMPLE_TITLE) and isLibraryRow, 20_000)
    }

    @After
    fun finish() {
        scenario?.close()
        ReadAloudSpeaker.make = { SystemSpeaker(it) }
    }

    private fun openReader() {
        compose.onNode(hasText(SAMPLE_TITLE) and isLibraryRow).performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.read"), 5_000)
        compose.onNodeWithTag("bookDetail.read").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("reader.block.0"), 10_000)
    }

    private fun textOf(tag: String): String =
        compose.onNodeWithTag(tag).fetchSemanticsNode().config.getOrNull(SemanticsProperties.Text)
            ?.joinToString("") { it.text } ?: ""

    private fun awaitText(tag: String, text: String) {
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasTestTag(tag) and hasText(text, substring = true)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    /** PL-9: Read Aloud says exactly the text the reader shows, block by block. */
    @Test
    fun pl9_readAloudSpeaksTheShownText() {
        openReader()
        assertEquals("The Storm", textOf("reader.title"))
        compose.onNodeWithTag("reader.readAloud").assertIsEnabled()
        saveScreenshot("pl9-reader")

        compose.onNodeWithTag("reader.readAloud").performClick()
        compose.waitUntil(5_000) { speaker.said.isNotEmpty() }
        compose.waitUntilExactlyOneExists(hasTestTag("reader.player.stop"), 5_000)
        assertEquals(textOf("reader.block.0"), speaker.said[0])
        saveScreenshot("pl9-reading-aloud")

        // The first block said to the end: the next one is the next block on the screen.
        InstrumentationRegistry.getInstrumentation().runOnMainSync { speaker.listener?.onDone(speaker.lastId) }
        compose.waitUntil(5_000) { speaker.said.size == 2 }
        assertEquals(textOf("reader.block.1"), speaker.said[1])
        assertEquals(
            "The storm arrived an hour after sunset, as the old keeper had said it would.",
            speaker.said[1],
        )

        compose.onNodeWithTag("reader.player.stop").performClick()
        compose.waitUntil(5_000) { compose.onAllNodes(hasTestTag("reader.player.stop")).fetchSemanticsNodes().isEmpty() }
        compose.onNodeWithTag("reader.readAloud").assertIsEnabled()

        compose.onNodeWithTag("reader.close").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.read"), 5_000)
    }

    /**
     * The reader moves between chapters by the progress bar and the contents, ends the book with
     * Mark as Finished, and opens again where it was left.
     */
    @Test
    fun theReaderMovesThroughTheBookAndKeepsThePlace() {
        openReader()
        assertEquals("Chapter 1 of 2 · 0%", textOf("reader.progress"))
        compose.onNodeWithTag("reader.nextChapter").performClick()
        awaitText("reader.title", "Morning")
        awaitText("reader.progress", "Chapter 2 of 2")
        compose.waitUntilExactlyOneExists(hasTestTag("reader.end"), 5_000)
        saveScreenshot("reader-end")

        compose.onNodeWithTag("reader.markFinished").performClick()
        compose.waitUntil(5_000) { compose.onAllNodes(hasTestTag("reader.markFinished")).fetchSemanticsNodes().isEmpty() }

        // Back to the first chapter through the contents, then on again.
        compose.onNodeWithTag("reader.contents").performClick()
        // No screenshot here: the sheet is a second window, which captureToImage can't take.
        compose.waitUntilExactlyOneExists(hasTestTag("contents.chapter.0"), 5_000)
        compose.onNodeWithTag("contents.chapter.0").performClick()
        awaitText("reader.title", "The Storm")
        compose.onNodeWithTag("reader.nextChapterEnd").performClick()
        awaitText("reader.title", "Morning")

        // Leaving saves the place: the book offers Continue Reading, and opens at Morning.
        compose.onNodeWithTag("reader.close").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.read") and hasText("Continue Reading"), 5_000)
        assertEquals("Finished", textOf("bookDetail.badge"))
        compose.onNodeWithTag("bookDetail.read").performClick()
        awaitText("reader.title", "Morning")
    }

    private fun saveScreenshot(name: String) {
        compose.waitForIdle()
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        PlatformTestStorageRegistry.getInstance().openOutputFile("screenshots/$name.png").use {
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    private companion object {
        /** The sample EPUB's title (examples/pocket-library/rust/src/sample.rs). */
        const val SAMPLE_TITLE = "The Lighthouse Keeper"
    }
}
