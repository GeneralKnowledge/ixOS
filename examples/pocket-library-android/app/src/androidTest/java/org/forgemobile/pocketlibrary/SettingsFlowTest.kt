package org.forgemobile.pocketlibrary

import android.content.Intent
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import androidx.work.workDataOf
import java.io.File
import java.util.UUID
import kotlinx.coroutines.runBlocking
import org.forgemobile.pocketlibrary.host.BackgroundTimeWorker
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.pocket_library_core.writeSampleEpub

/**
 * Settings on an emulator (A6), on a throwaway library that starts with the sample EPUB, which
 * itself arrives through the import job. Backups and spreadsheets go through the system's file
 * pickers, which a Compose test can't drive; the JVM tests cover them against the Rust core.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class SettingsFlowTest {
    private val isLibraryRow = SemanticsMatcher("is a library.book row") { node ->
        node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("library.book.") == true
    }

    @get:Rule
    val compose = createEmptyComposeRule()

    private var scenario: ActivityScenario<MainActivity>? = null
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val app get() = context.applicationContext as PocketLibraryApplication

    @Before
    fun launch() {
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, true)
            .putExtra(PocketLibraryApplication.SAMPLE_BOOK_EXTRA, true)
        scenario = ActivityScenario.launch(intent)
        compose.waitUntilAtLeastOneExists(hasText(SAMPLE_TITLE) and isLibraryRow, 20_000)
    }

    @After
    fun finish() {
        scenario?.close()
    }

    private fun openSettings() {
        compose.onNodeWithTag("library.settings").performClick()
        compose.waitUntilExactlyOneExists(hasTestTag("settings.books"), 5_000)
    }

    private fun awaitValue(tag: String, value: String) {
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasTestTag(tag) and hasText(value)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun rowTitles(): List<String> =
        compose.onAllNodes(isLibraryRow).fetchSemanticsNodes().map { node ->
            node.config.getOrNull(SemanticsProperties.Text)?.joinToString(" ") { it.text } ?: ""
        }

    /** Settings shows what the library holds: the sample book and its saved EPUB. */
    @Test
    fun settingsShowWhatTheLibraryHolds() {
        openSettings()
        awaitValue("settings.books", "1")
        awaitValue("settings.savedBooks", "1")
        compose.onNodeWithTag("settings.builtWith").performScrollTo()
        awaitValue("settings.bookData", "Open Library")
    }

    /** The default order is saved in Rust and the library follows it as soon as it is chosen. */
    @Test
    fun theDefaultSortOrdersTheLibrary() {
        runBlocking { app.model.add(LibraryModel.emptyInput().copy(title = "Zorba the Greek")) }
        compose.waitUntil(5_000) { rowTitles().size == 2 }
        // Recently added first.
        assertTrue(rowTitles().first().contains("Zorba the Greek"))

        openSettings()
        compose.onNodeWithTag("settings.sort").performClick()
        compose.onNodeWithTag("settings.sort.title").performClick()
        compose.waitUntil(5_000) { app.model.settings.value.defaultSort == uniffi.pocket_library_core.LibrarySort.TITLE }
        compose.onNodeWithTag("settings.back").performClick()

        compose.waitUntil(5_000) { rowTitles().firstOrNull()?.contains(SAMPLE_TITLE) == true }
    }

    /** Erasing asks first; Cancel keeps the library, Erase Library empties it. */
    @Test
    fun erasingAsksFirst() {
        openSettings()
        compose.onNodeWithTag("settings.erase").performScrollTo().performClick()
        compose.onNodeWithTag("settings.erase.cancel").performClick()
        awaitValue("settings.books", "1")

        compose.onNodeWithTag("settings.erase").performScrollTo().performClick()
        compose.onNodeWithTag("settings.erase.confirm").performClick()
        awaitValue("settings.books", "0")
        compose.onNodeWithTag("settings.back").performClick()
        compose.waitUntil(5_000) { rowTitles().isEmpty() }
    }

    /**
     * The import job: it imports the EPUB waiting beside the library, deletes the copy, and does
     * nothing for a file that has gone (its throwaway library was emptied).
     */
    @Test
    fun anImportJobAddsTheWaitingBook() = runBlocking {
        app.model.eraseLibrary()
        val env = app.host.openedEnvironment!!
        val folder = File(EpubImports.pendingFolder(env), UUID.randomUUID().toString()).apply { mkdirs() }
        val file = File(folder, "Sample Book.epub")
        writeSampleEpub(file.path)

        assertEquals(ListenableWorker.Result.success(), runImport(file))
        app.model.reload()
        assertEquals(listOf(SAMPLE_TITLE), app.model.all.value.map { it.title })
        assertFalse(folder.exists())

        // Already done, or gone with its library: nothing to do.
        assertEquals(ListenableWorker.Result.success(), runImport(file))
        assertEquals(1, app.model.all.value.size)
    }

    /** Background time held by a process that has since ended has nothing to wait for. */
    @Test
    fun aBackgroundTimeJobFromAnEndedProcessEndsAtOnce() = runBlocking {
        val worker = TestListenableWorkerBuilder<BackgroundTimeWorker>(context)
            .setInputData(workDataOf("hold" to "an-earlier-process"))
            .build()
        assertEquals(ListenableWorker.Result.success(), worker.doWork())
    }

    private suspend fun runImport(file: File): ListenableWorker.Result =
        TestListenableWorkerBuilder<EpubImportWorker>(context)
            .setInputData(workDataOf(EpubImportWorker.PATH to file.path, EpubImportWorker.TEMPORARY to true))
            .build()
            .doWork()

    private companion object {
        /** The sample EPUB's title (examples/pocket-library/rust/src/sample.rs). */
        const val SAMPLE_TITLE = "The Lighthouse Keeper"
    }
}
