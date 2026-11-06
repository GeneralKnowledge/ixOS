package FORGE_JAVA_PACKAGE

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.delay
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import uniffi.forge_ffi.AppException

/**
 * Drives the view model against the app's real Rust core (rust/) through the generated Kotlin
 * bindings. `forge test android --project .` builds the host library first (`cargo build
 * --manifest-path rust/Cargo.toml`).
 */
class NotesViewModelTest {
    private lateinit var dir: File

    @Before
    fun setUp() {
        dir = Files.createTempDirectory("forge-notes-test").toFile()
    }

    @After
    fun tearDown() {
        dir.deleteRecursively()
    }

    private fun newModel() = NotesViewModel(dir.absolutePath)

    @Test
    fun notesAreSavedAndSurviveARestart() = runBlocking {
        val model = newModel()
        assertNull(model.openError)
        assertEquals(emptyList<Any>(), model.notes)

        model.draft = "first"
        model.addNote()
        model.draft = "second"
        model.addNote()
        assertEquals(listOf("second", "first"), model.notes.map { it.text })
        assertEquals("", model.draft)
        model.close()

        val reopened = newModel()
        assertEquals(listOf("second", "first"), reopened.notes.map { it.text })
        reopened.delete(reopened.notes[1])
        assertEquals(listOf("second"), reopened.notes.map { it.text })
        reopened.close()
    }

    @Test
    fun rustErrorsArriveTyped() = runBlocking {
        val model = newModel()
        model.draft = "   "
        model.addNote()
        assertNotNull(model.errorMessage)
        assertTrue(model.notes.isEmpty())

        try {
            model.core!!.deleteNote(42uL)
            fail("expected NotFound")
        } catch (e: AppException.NotFound) {
            assertEquals(42uL, e.id)
        }
        model.close()
    }

    @Test
    fun changesMadeInRustReachTheModelThroughEvents() = runBlocking {
        val model = newModel()
        val listener = launch { model.listen() }
        model.core!!.addNote("from Rust")
        withTimeout(5_000) {
            while (model.notes.isEmpty()) delay(10)
        }
        assertEquals(listOf("from Rust"), model.notes.map { it.text })
        // Closing ends the stream, so the listener returns on its own.
        model.close()
        withTimeout(5_000) { listener.join() }
    }

    @Test
    fun unreadableDataIsReportedNotOverwritten() {
        val file = File(dir, "notes.json")
        val newer = """{"schema_version": 99, "data": {}}"""
        file.writeText(newer)
        val model = newModel()
        assertNotNull(model.openError)
        assertNull(model.core)
        assertEquals(newer, file.readText())
    }

    @Test
    fun closedCoreRefusesWork() = runBlocking {
        val model = newModel()
        val core = model.core!!
        core.shutdown()
        try {
            core.addNote("late")
            fail("expected ShutDown")
        } catch (_: AppException.ShutDown) {
        }
        model.close()
    }
}
