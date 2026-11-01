package org.forgemobile.counter

import java.io.File
import java.nio.file.Files
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Drives the view model against the real Rust core through the generated Kotlin bindings.
 * Needs the host library: run `cargo build -p forge-ffi` from the workspace root first.
 */
class CounterViewModelTest {
    private lateinit var dir: File
    private val snapshotFile get() = File(dir, "forge-counter-v1.json")

    @Before
    fun setUp() {
        dir = Files.createTempDirectory("forge-counter-test").toFile()
    }

    @After
    fun tearDown() {
        dir.deleteRecursively()
    }

    private fun newModel() = CounterViewModel(dir.absolutePath)

    @Test
    fun freshInstallStartsAtZero() {
        val model = newModel()
        assertFalse(model.isBlocked)
        assertEquals(0L, model.count)
        assertEquals("Fresh install", model.startupLabel)
        assertTrue(model.greeting.isNotEmpty())
    }

    @Test
    fun actionsPersistAcrossInstances() {
        val model = newModel()
        model.increment()
        model.increment()
        model.decrement()
        assertEquals(1L, model.count)
        assertNull(model.errorMessage)

        val reopened = newModel()
        assertEquals(1L, reopened.count)
        assertEquals("Restored saved state", reopened.startupLabel)
    }

    @Test
    fun decrementAtZeroShowsErrorAndKeepsCount() {
        val model = newModel()
        model.decrement()
        assertEquals(0L, model.count)
        assertNotNull(model.errorMessage)
    }

    @Test
    fun corruptDataBlocksAndCanBeReset() {
        snapshotFile.writeText("{not json")
        val model = newModel()
        assertTrue(model.isBlocked)
        assertTrue(model.canRecoverFromCorruption)

        model.recoverFromCorruption()
        assertFalse(model.isBlocked)
        assertEquals(0L, model.count)
        assertEquals("Recovered — fresh counter", model.startupLabel)
    }

    @Test
    fun newerSchemaIsNotOfferedForReset() {
        val newer = """{"schema_version": 999, "count": 7}"""
        snapshotFile.writeText(newer)
        val model = newModel()
        assertTrue(model.isBlocked)
        assertFalse(model.canRecoverFromCorruption)
        assertTrue(model.errorMessage!!.contains("newer version"))

        // Even if a host called recovery anyway, Rust refuses to overwrite the data.
        model.recoverFromCorruption()
        assertTrue(model.isBlocked)
        assertEquals(newer, snapshotFile.readText())
    }
}
