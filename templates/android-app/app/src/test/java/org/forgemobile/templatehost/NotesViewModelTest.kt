package FORGE_JAVA_PACKAGE

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.cancelAndJoin
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
import androidx.lifecycle.Lifecycle
import uniffi.forge_ffi.AppEvent
import uniffi.forge_ffi.AppException
import uniffi.forge_ffi.AppPhase

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
    fun cancellingTheCoroutineStopsTheRequestInRust() = runBlocking {
        // Accepts connections and never answers: only a cancel ends a request to it.
        java.net.ServerSocket(0, 4, java.net.InetAddress.getByName("127.0.0.1")).use { server ->
            val model = newModel()
            model.url = "http://127.0.0.1:${server.localPort}/"
            val started = System.nanoTime()
            val check = launch { model.checkUrl() }
            delay(200)
            check.cancelAndJoin()
            assertEquals("Cancelled.", model.urlResult)
            assertTrue(System.nanoTime() - started < 5_000_000_000L)
            assertTrue(!model.isCheckingUrl)
            // The core still works afterwards.
            model.draft = "after"
            model.addNote()
            assertEquals(listOf("after"), model.notes.map { it.text })
            model.close()
        }
    }

    @Test
    fun goingToTheBackgroundInterruptsTheRequestInRust() = runBlocking {
        java.net.ServerSocket(0, 4, java.net.InetAddress.getByName("127.0.0.1")).use { server ->
            val model = newModel()
            model.setPhase(AppPhase.ACTIVE)
            model.url = "http://127.0.0.1:${server.localPort}/"
            val started = System.nanoTime()
            val check = launch { model.checkUrl() }
            delay(200)
            model.setPhase(AppPhase.BACKGROUND)
            withTimeout(5_000) { check.join() }
            assertEquals(NotesViewModel.describe(AppException.Interrupted()), model.urlResult)
            assertTrue(System.nanoTime() - started < 5_000_000_000L)
            assertEquals(AppPhase.BACKGROUND, model.core!!.phase())
            model.close()
        }
    }

    @Test
    fun resumeIsReportedAndTerminateClosesTheCore() = runBlocking {
        val model = newModel()
        val core = model.core!!
        val events = core.events()
        model.setPhase(AppPhase.ACTIVE)
        model.setPhase(AppPhase.BACKGROUND)
        model.setPhase(AppPhase.INACTIVE)
        assertEquals(AppEvent.PhaseChanged(AppPhase.ACTIVE, false), events.next())
        assertEquals(AppEvent.PhaseChanged(AppPhase.BACKGROUND, false), events.next())
        assertEquals(AppEvent.PhaseChanged(AppPhase.INACTIVE, true), events.next())

        model.setPhase(AppPhase.TERMINATED)
        assertNull(model.core)
        try {
            core.addNote("late")
            fail("expected ShutDown")
        } catch (_: AppException.ShutDown) {
        }
    }

    @Test
    fun activityEventsMapOntoAppPhases() {
        assertEquals(AppPhase.INACTIVE, appPhaseFor(Lifecycle.Event.ON_START, false))
        assertEquals(AppPhase.ACTIVE, appPhaseFor(Lifecycle.Event.ON_RESUME, false))
        assertEquals(AppPhase.INACTIVE, appPhaseFor(Lifecycle.Event.ON_PAUSE, false))
        assertEquals(AppPhase.BACKGROUND, appPhaseFor(Lifecycle.Event.ON_STOP, false))
        // Rotation stops and restarts the activity; that is not the background.
        assertNull(appPhaseFor(Lifecycle.Event.ON_STOP, true))
        assertNull(appPhaseFor(Lifecycle.Event.ON_CREATE, false))
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
