package org.forgemobile.pocketlibrary

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onRoot
import androidx.test.platform.io.PlatformTestStorageRegistry
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Pocket Library's requirements on an emulator, named by their ids in
 * examples/pocket-library/requirements.toml. Controls are found by the iOS app's identifiers
 * (`screen.element`), never by position. Each test launches on an empty throwaway library, so
 * the person's own library is never touched and tests don't see each other's books.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class LibraryFlowTest {
    /** A row of the library list: `library.book.<id>`, the id coming from Rust. */
    private val isLibraryRow = SemanticsMatcher("is a library.book row") { node ->
        node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("library.book.") == true
    }

    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null

    /**
     * Launches on throwaway test data, emptied first unless [keep] (a relaunch that checks
     * something was saved); [unreadable] starts it as a file that isn't a library.
     */
    private fun launch(keep: Boolean = false, unreadable: Boolean = false) {
        scenario?.close()
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, true)
            .putExtra(ForgeEnvironment.KEEP_TEST_DATA_EXTRA, keep)
            .putExtra(PocketLibraryApplication.UNREADABLE_TEST_DATA_EXTRA, unreadable)
        scenario = ActivityScenario.launch(intent)
    }

    @After
    fun finish() {
        scenario?.close()
    }

    /** The core opens off the main thread; a failure to open shows `open.error`. */
    private fun awaitLibrary(tag: String = "library.empty") {
        compose.waitUntilAtLeastOneExists(hasTestTag(tag) or hasTestTag("open.error"), 20_000)
        val failure = compose.onAllNodes(hasTestTag("open.error")).fetchSemanticsNodes().firstOrNull()
        if (failure != null) {
            val text = failure.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }
            throw AssertionError("The library didn't open: $text")
        }
    }

    private fun launchEmpty() {
        launch()
        awaitLibrary()
    }

    private fun row(title: String) = hasText(title) and isLibraryRow

    /** Adds a book from the library; saving opens it, as on the iPhone. */
    private fun addBook(title: String, author: String = "", currentPage: String = "", totalPages: String = "") {
        val empty = compose.onAllNodes(hasTestTag("library.empty.add")).fetchSemanticsNodes().isNotEmpty()
        compose.onNodeWithTag(if (empty) "library.empty.add" else "library.add.manual").performClick()
        compose.onNodeWithTag("bookForm.title").performTextInput(title)
        if (author.isNotEmpty()) compose.onNodeWithTag("bookForm.author").performTextInput(author)
        if (currentPage.isNotEmpty()) compose.onNodeWithTag("bookForm.currentPage").performTextInput(currentPage)
        if (totalPages.isNotEmpty()) compose.onNodeWithTag("bookForm.totalPages").performTextInput(totalPages)
        compose.onNodeWithTag("bookForm.save").assertIsEnabled().performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.title") and hasText(title), 5_000)
    }

    private fun backToLibrary() {
        compose.onNodeWithTag("bookDetail.back").performClick()
        compose.waitUntilDoesNotExist(hasTestTag("bookDetail.title"), 5_000)
    }

    @Test
    fun addOpenAndDeleteABook() {
        launchEmpty()
        addBook("A Wizard of Earthsea", "Ursula K. Le Guin")
        compose.onNodeWithTag("bookDetail.status").assertTextEquals("Want to Read")
        backToLibrary()

        // Back on the library, the new book is a row.
        compose.waitUntilExactlyOneExists(row("A Wizard of Earthsea"), 5_000)
        compose.onNode(row("A Wizard of Earthsea")).performClick()

        compose.onNodeWithTag("bookDetail.title").assertTextEquals("A Wizard of Earthsea")
        compose.onNodeWithTag("bookDetail.delete").performScrollTo().performClick()
        compose.onNodeWithTag("bookDetail.delete.confirm").performClick()

        compose.waitUntilExactlyOneExists(hasTestTag("library.empty"), 5_000)
    }

    /** PL-1 and PL-2: a page on an unread book starts it, and progress survives a relaunch. */
    @Test
    fun pl1_pl2_progressStartsABookAndSurvivesARelaunch() {
        launchEmpty()
        addBook("Dune", totalPages = "400")
        compose.onNodeWithTag("bookDetail.status").assertTextEquals("Want to Read")

        // The keyboard's Done saves the page.
        compose.onNodeWithTag("bookDetail.page").performTextReplacement("100")
        compose.onNodeWithTag("bookDetail.page").performImeAction()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.status") and hasText("Reading"), 5_000)
        compose.onNodeWithTag("bookDetail.progress").assertTextEquals("25% · page 100 of 400")
        saveScreenshot("pl2-reading")
        backToLibrary()
        compose.waitUntilExactlyOneExists(row("Dune"), 5_000)

        // A new launch on the same test data opens the library from disk again.
        launch(keep = true)
        awaitLibrary(tag = "library.search")
        compose.waitUntilExactlyOneExists(row("Dune"), 5_000)
        compose.onNode(row("Dune")).performClick()
        compose.onNodeWithTag("bookDetail.page").assert(hasText("100"))
        compose.onNodeWithTag("bookDetail.status").assertTextEquals("Reading")
    }

    /**
     * PL-3 and PL-6: search narrows the list; finishing a book on its own screen reaches the
     * library, where the Finished filter shows it.
     */
    @Test
    fun pl3_pl6_searchFilterAndAChangeFromAnotherScreen() {
        launchEmpty()
        addBook("Dune", "Frank Herbert", currentPage = "10")
        backToLibrary()
        addBook("Emma", "Jane Austen")
        backToLibrary()

        // PL-3: search.
        compose.onNodeWithTag("library.search").performTextInput("Emma")
        compose.waitUntilDoesNotExist(row("Dune"), 5_000)
        compose.onNode(row("Emma")).assertIsDisplayed()
        saveScreenshot("pl3-search")
        compose.onNodeWithTag("library.search.clear").performClick()
        compose.waitUntilExactlyOneExists(row("Dune"), 5_000)

        // PL-6: finish Dune on its own screen.
        compose.onNode(row("Dune")).performClick()
        compose.onNodeWithTag("bookDetail.markFinished").performScrollTo().performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.status") and hasText("Finished"), 5_000)
        backToLibrary()

        // PL-3: the Finished filter shows only Dune.
        compose.onNodeWithTag("library.filter").performClick()
        compose.onNodeWithTag("library.filter.finished").performClick()
        compose.waitUntilDoesNotExist(row("Emma"), 5_000)
        compose.onNode(row("Dune")).assertIsDisplayed()
        saveScreenshot("pl3-finished-filter")

        // Nothing is Want to Read: the filter says so.
        compose.onNodeWithTag("library.filter").performClick()
        compose.onNodeWithTag("library.filter.wantToRead").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("library.filtered.empty") and hasText("No Want to Read Books"), 5_000)

        // And the sort order is a choice in the same menu: newest first, then by title.
        compose.onNodeWithTag("library.filter").performClick()
        compose.onNodeWithTag("library.filter.all").performClick()
        compose.waitUntilExactlyOneExists(row("Emma"), 5_000)
        assertEquals(listOf("Emma", "Dune"), rowTitles())
        compose.onNodeWithTag("library.filter").performClick()
        compose.onNodeWithTag("library.sort.title").performClick()
        compose.waitUntil(5_000) { rowTitles() == listOf("Dune", "Emma") }
    }

    /** The library's rows, top to bottom, by title. */
    private fun rowTitles(): List<String> = compose.onAllNodes(isLibraryRow).fetchSemanticsNodes()
        .sortedBy { it.boundsInRoot.top }
        .map { node -> node.config.getOrNull(SemanticsProperties.Text)?.firstOrNull()?.text ?: "" }

    /** Saved with the run's test output (the CI artifact `pocket-library-android-screenshots`). */
    private fun saveScreenshot(name: String) {
        compose.waitForIdle()
        val bitmap = compose.onRoot().captureToImage().asAndroidBitmap()
        PlatformTestStorageRegistry.getInstance().openOutputFile("screenshots/$name.png").use {
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    /** PL-5: an unreadable library can be started fresh, after asking first. */
    @Test
    fun pl5_anUnreadableLibraryCanBeStartedFresh() {
        launch(unreadable = true)
        compose.waitUntilExactlyOneExists(hasTestTag("open.startFresh"), 20_000)
        compose.onNodeWithTag("open.error.title").assertTextEquals("Can't Open Library")
        saveScreenshot("pl5-cannot-open")
        compose.onNodeWithTag("open.startFresh").performClick()
        compose.onNodeWithTag("open.startFresh.confirm").assertTextEquals("Start New Library").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("library.empty"), 20_000)
    }

    /** PL-10: the New Book form takes a typed title, enables Add, and Cancel leaves without saving. */
    @Test
    fun pl10_newBookFormEnablesAddAndCancelSavesNothing() {
        launchEmpty()
        compose.onNodeWithTag("library.empty.add").performClick()
        compose.onNodeWithTag("bookForm.save").assertIsNotEnabled()
        compose.onNodeWithTag("bookForm.title").performTextInput("Hello from Forge")
        compose.onNodeWithTag("bookForm.title").assert(hasText("Hello from Forge"))
        compose.onNodeWithTag("bookForm.save").assertIsEnabled()

        compose.onNodeWithTag("bookForm.cancel").performClick()
        compose.waitUntilDoesNotExist(hasTestTag("bookForm.title"), 5_000)
        compose.onNodeWithTag("library.empty").assertIsDisplayed()
    }

    /** PL-4: invalid details are refused in plain language under the field, and change nothing. */
    @Test
    fun pl4_invalidDetailsAreRefusedAndChangeNothing() {
        launchEmpty()
        compose.onNodeWithTag("library.empty.add").performClick()
        compose.onNodeWithTag("bookForm.title").performTextInput("Short Book")
        compose.onNodeWithTag("bookForm.currentPage").performTextInput("50")
        compose.onNodeWithTag("bookForm.totalPages").performTextInput("10")
        compose.onNodeWithTag("bookForm.save").performClick()

        val message = hasTestTag("bookForm.currentPage.error") and
            hasContentDescription("Error: The current page is past the last page (10).")
        compose.waitUntilExactlyOneExists(message, 5_000)
        // The form stays open with what was typed.
        compose.onNodeWithTag("bookForm.save").assertIsEnabled()
        compose.onNodeWithTag("bookForm.title").assert(hasText("Short Book"))

        compose.onNodeWithTag("bookForm.cancel").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("library.empty"), 5_000)
    }

    /** Edit mode: the form starts from the book and Save changes it. */
    @Test
    fun editingABookChangesIt() {
        launchEmpty()
        addBook("Piranesi")
        compose.onNodeWithTag("bookDetail.edit").performClick()
        compose.onNodeWithTag("bookForm.title").assert(hasText("Piranesi"))
        compose.onNodeWithTag("bookForm.save").assertTextEquals("Save")
        compose.onNodeWithTag("bookForm.author").performTextInput("Susanna Clarke")
        compose.onNodeWithTag("bookForm.star.4").performScrollTo().performClick()
        compose.onNodeWithTag("bookForm.save").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("bookDetail.title"), 5_000)
        compose.onNode(hasText("Susanna Clarke")).assertIsDisplayed()
        compose.onNodeWithTag("bookDetail.star.4").performScrollTo().assert(SemanticsMatcher.expectValue(SemanticsProperties.Selected, true))
    }
}
