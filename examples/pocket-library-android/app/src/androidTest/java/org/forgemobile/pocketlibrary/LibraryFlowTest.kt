package org.forgemobile.pocketlibrary

import android.content.Intent
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.printToLog
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Stage 0's end-to-end check on an emulator: add a book, open it, delete it. Controls are found
 * by the iOS app's identifiers (`screen.element`), never by position. The app starts on
 * throwaway data, so the person's own library is never touched.
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

    private lateinit var scenario: ActivityScenario<MainActivity>

    @Before
    fun launch() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, true)
        scenario = ActivityScenario.launch(intent)
    }

    @After
    fun finish() = scenario.close()

    @Test
    fun addOpenAndDeleteABook() {
        // The core opens off the main thread; a failure to open shows `open.error`.
        compose.waitUntilAtLeastOneExists(hasTestTag("library.empty") or hasTestTag("open.error"), 20_000)
        if (compose.onAllNodes(hasTestTag("open.error")).fetchSemanticsNodes().isNotEmpty()) {
            compose.onRoot().printToLog("LibraryFlowTest")
            throw AssertionError("The library didn't open; see the open.error text in the log above")
        }

        compose.onNodeWithTag("library.add.manual").performClick()
        compose.onNodeWithTag("bookForm.save").assertIsNotEnabled()
        compose.onNodeWithTag("bookForm.title").performTextInput("A Wizard of Earthsea")
        compose.onNodeWithTag("bookForm.author").performTextInput("Ursula K. Le Guin")
        compose.onNodeWithTag("bookForm.save").assertIsEnabled().performClick()

        // Back on the library, the new book is a row.
        val row = hasText("A Wizard of Earthsea") and isLibraryRow
        compose.waitUntilExactlyOneExists(row, 5_000)
        compose.onNode(row).performClick()

        compose.onNodeWithTag("bookDetail.title").assertTextEquals("A Wizard of Earthsea")
        compose.onNodeWithTag("bookDetail.status").assertTextEquals("Want to Read")
        compose.onNodeWithTag("bookDetail.delete").performClick()
        compose.onNodeWithTag("bookDetail.delete.confirm").performClick()

        compose.waitUntilExactlyOneExists(hasTestTag("library.empty"), 5_000)
    }
}
