package org.forgemobile.pocketlibrary.host

import java.nio.file.Files
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.forge_app.AppPhase
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvent
import uniffi.forge_app.RuntimeEvents

/**
 * The host asks for background time while Rust has work to finish after the app leaves the
 * screen, and gives it back when the work ends or the app returns (ForgeKit's ForgeHost does the
 * same with beginBackgroundTask). A fake core says when work is pending; a fake time records
 * what the host asked for.
 */
class ForgeHostBackgroundTimeTest {
    private val root = Files.createTempDirectory("forge-host").toFile()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val core = FakeCore()
    private val time = FakeTime()
    private lateinit var host: ForgeHost<FakeCore>

    @Before
    fun open() = runBlocking {
        host = ForgeHost(scope = scope, environment = { ForgeEnvironment.temporary(root) }, open = { core })
        host.backgroundTime = time
        host.start()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
    }

    @After
    fun close() {
        host.close()
        scope.cancel()
        root.deleteRecursively()
    }

    @Test
    fun nothingPendingAsksForNoTime() {
        host.report(AppPhase.BACKGROUND)
        assertEquals(0, time.begun)
        assertFalse(host.holdsBackgroundTime)
    }

    @Test
    fun pendingWorkHoldsTimeUntilTheAppReturns() {
        core.pending = true
        host.report(AppPhase.INACTIVE)
        host.report(AppPhase.BACKGROUND)
        assertEquals(1, time.begun)
        assertTrue(host.holdsBackgroundTime)
        // Asked once, however often the phase is reported.
        host.report(AppPhase.BACKGROUND)
        assertEquals(1, time.begun)

        host.report(AppPhase.INACTIVE)
        assertEquals(1, time.ended)
        assertFalse(host.holdsBackgroundTime)
    }

    @Test
    fun rustSaysWhenWorkStartsAndEndsInTheBackground() {
        host.report(AppPhase.BACKGROUND)
        host.handle(RuntimeEvent.BackgroundWork(pending = true))
        assertEquals(1, time.begun)
        host.handle(RuntimeEvent.BackgroundWork(pending = false))
        assertEquals(1, time.ended)
        assertFalse(host.holdsBackgroundTime)

        // On screen, work needs no background time.
        host.report(AppPhase.ACTIVE)
        host.handle(RuntimeEvent.BackgroundWork(pending = true))
        assertEquals(1, time.begun)
    }

    @Test
    fun timeRunningOutStopsRustsFinishingWork() {
        core.pending = true
        host.report(AppPhase.BACKGROUND)
        val firstExpiry = time.expiry!!
        firstExpiry()
        assertEquals(1, core.expired)
        assertEquals(1, time.ended)
        assertFalse(host.holdsBackgroundTime)

        // The first hold's expiry, arriving late, doesn't end a newer hold.
        host.report(AppPhase.ACTIVE)
        host.report(AppPhase.BACKGROUND)
        assertEquals(2, time.begun)
        firstExpiry()
        assertEquals(1, core.expired)
        assertTrue(host.holdsBackgroundTime)
    }

    @Test
    fun closingGivesTheTimeBack() {
        core.pending = true
        host.report(AppPhase.BACKGROUND)
        host.close()
        assertEquals(1, time.ended)
        assertFalse(host.holdsBackgroundTime)
    }

    class FakeCore : ForgeCore {
        @Volatile var pending = false
        @Volatile var expired = 0

        override fun setPhase(phase: AppPhase) = Unit
        override fun events(): RuntimeEvents? = null
        override fun revision(): ULong = 0u
        override fun backgroundWorkPending(): Boolean = pending
        override fun backgroundTimeExpired() {
            expired += 1
        }
        override fun launchReport(): LaunchReport = LaunchReport(unfinishedBackgroundWork = false)
        override fun shutdown() = Unit
        override fun close() = Unit
    }

    class FakeTime : ForgeBackgroundTime {
        var begun = 0
        var ended = 0
        var expiry: (() -> Unit)? = null

        override fun begin(onExpiry: () -> Unit) {
            begun += 1
            expiry = onExpiry
        }

        override fun end() {
            ended += 1
        }
    }
}
