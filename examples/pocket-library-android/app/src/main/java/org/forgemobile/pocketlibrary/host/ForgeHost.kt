// The Android counterpart of ForgeKit's ForgeHost (forgekit/Sources/ForgeKit/ForgeHost.swift):
// owns an app's Rust core for the life of the process, drives the forge-app runtime contract
// and tells the UI when Rust's state may have changed.
//
// Plain Kotlin and coroutines, no Android UI, so it runs in JVM unit tests against the host
// build of the Rust library. It lives in this app until a second Android app needs it
// (docs/architecture.md#framework-principles); then it moves into a ForgeKit for Android.
//
// Not here yet, compared with the Swift host: asking the system for time to finish work in
// the background (iOS beginBackgroundTask). Android has no direct equivalent; that is part of
// the services stage of the Android plan.
package org.forgemobile.pocketlibrary.host

import java.io.File
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.forge_app.AppPhase
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvent
import uniffi.forge_app.RuntimeEvents
import uniffi.forge_app.forgeAppContractVersion

/** The version of the forge-app contract this host was written against. */
const val FORGE_HOST_CONTRACT_VERSION: UInt = 3u

/**
 * What the host needs from an app's Rust core: the runtime contract every forge-app core
 * exports (`forge_app::export_runtime!`). Kotlin can't add an interface to a generated class,
 * so an app wraps its core in one line of delegation (see `LibraryCore`).
 */
interface ForgeCore : AutoCloseable {
    fun setPhase(phase: AppPhase)
    fun events(): RuntimeEvents?
    fun revision(): ULong
    fun launchReport(): LaunchReport
    fun shutdown()

    /**
     * Frees the Rust object. Kotlin doesn't free it when the last reference goes (it waits for
     * the garbage collector), so the host calls this after [shutdown]: until then the core
     * still holds its data directory, and opening it again would fail.
     */
    override fun close()
}

/** Directories the runtime gives the core. */
data class ForgeEnvironment(val dataDirectory: String, val cachesDirectory: String) {
    companion object {
        /**
         * The launch extra that starts the app on throwaway data, so UI tests never touch the
         * person's own data. The Android form of iOS's `-ForgeTemporaryData` argument.
         */
        const val TEMPORARY_DATA_EXTRA = "forge.temporaryData"

        /**
         * With [TEMPORARY_DATA_EXTRA], keeps the last launch's throwaway data instead of
         * emptying it: a relaunch that checks something was saved. iOS's `-ForgeKeepTestData`.
         */
        const val KEEP_TEST_DATA_EXTRA = "forge.keepTestData"

        /**
         * `<files>/<app>/data` and `<cache>/<app>`, or, with [temporary], a folder under the
         * cache that each launch empties unless it [keep]s it.
         */
        fun standard(
            app: String,
            files: File,
            cache: File,
            temporary: Boolean,
            keep: Boolean = false,
        ): ForgeEnvironment {
            if (temporary) {
                val base = File(cache, "forge-test-$app")
                if (!keep) base.deleteRecursively()
                return make(File(base, "data"), File(base, "caches"))
            }
            return make(File(File(files, app), "data"), File(cache, app))
        }

        /** A fresh throwaway environment under [root], for tests. */
        fun temporary(root: File): ForgeEnvironment {
            val base = File(root, "forge-${java.util.UUID.randomUUID()}")
            return make(File(base, "data"), File(base, "caches"))
        }

        private fun make(data: File, caches: File): ForgeEnvironment {
            data.mkdirs()
            caches.mkdirs()
            return ForgeEnvironment(data.absolutePath, caches.absolutePath)
        }
    }
}

class ForgeHost<Core : ForgeCore>(
    private val scope: CoroutineScope,
    private val environment: () -> ForgeEnvironment,
    private val open: (ForgeEnvironment) -> Core,
    /** Where the core is opened (it reads files). Tests may pass their own. */
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    sealed interface Status {
        data object Loading : Status
        data object Ready : Status
        /** Opening failed; [error] is what the core threw, when it threw. */
        data class Failed(val message: String, val error: Throwable? = null) : Status
        /** Shut down; [start] opens the core again. */
        data object Closed : Status
    }

    private val _status = MutableStateFlow<Status>(Status.Loading)
    val status: StateFlow<Status> = _status.asStateFlow()

    /**
     * Increases whenever Rust's state may have changed (a commit, missed events, a return from
     * the background). Models re-query when it moves.
     */
    private val _changes = MutableStateFlow(0L)
    val changes: StateFlow<Long> = _changes.asStateFlow()

    /** The last committed revision Rust reported. */
    var revision: ULong = 0u
        private set

    var phase: AppPhase = AppPhase.LAUNCHING
        private set

    /** What the runtime found when the core opened. */
    var launch: LaunchReport? = null
        private set

    @Volatile
    var core: Core? = null
        private set

    /** Read by the listener's pool thread too: it tells a closed stream from a broken one. */
    @Volatile
    private var stream: RuntimeEvents? = null
    private var listener: Job? = null
    private var opening = false

    /** Opens the core off the main thread and starts the event loop. Does nothing once open. */
    suspend fun start() {
        if (core != null || opening) return
        opening = true
        try {
            _status.value = Status.Loading
            val linked = try {
                forgeAppContractVersion()
            } catch (e: Throwable) {
                // The native library didn't load (a missing ABI, JNA not packaged).
                _status.value = Status.Failed("The app's Rust library couldn't be loaded: $e")
                return
            }
            if (linked != FORGE_HOST_CONTRACT_VERSION) {
                _status.value = Status.Failed(
                    "This build mixes framework versions (Rust contract $linked, host $FORGE_HOST_CONTRACT_VERSION).",
                )
                return
            }
            val opened = try {
                withContext(io) { open(environment()) }
            } catch (e: CancellationException) {
                // Whoever started it went away; that isn't a failure to open. Start again later.
                _status.value = Status.Closed
                throw e
            } catch (e: Exception) {
                _status.value = Status.Failed(e.message ?: "The app's data couldn't be opened ($e).", e)
                return
            }
            // Subscribe before anyone reads, so no change is missed.
            val events = opened.events()
            core = opened
            stream = events
            launch = opened.launchReport()
            if (events != null) listen(events)
            if (phase != AppPhase.LAUNCHING) opened.setPhase(phase)
            _status.value = Status.Ready
            reconcile()
        } finally {
            opening = false
        }
    }

    /** Reports the application lifecycle. The activity calls this; tests call it directly. */
    fun report(phase: AppPhase) {
        this.phase = phase
        if (phase == AppPhase.TERMINATED) {
            close()
            return
        }
        core?.setPhase(phase)
    }

    /** Ends the event loop and shuts the core down. Committed work is already on disk. */
    fun close() {
        val events = stream
        // Cleared first, so a listener that finds the stream freed knows it was closed.
        stream = null
        events?.cancel()
        listener?.cancel()
        core?.shutdown()
        core?.close()
        // Freed now, not when the listener ends: the stream keeps the runtime, and with it the
        // data folder's lock, until it is freed, and a reopen must find the folder free.
        events?.close()
        listener = null
        core = null
        _status.value = Status.Closed
    }

    /** Takes the revision from Rust after anything that may have skipped a `Changed`. */
    private fun reconcile() {
        core?.let { revision = it.revision() }
        _changes.value += 1
    }

    private fun listen(events: RuntimeEvents) {
        // Awaited on a pool thread, not the caller's (often main) dispatcher: UniFFI resumes
        // from Rust's waker, and a dispatcher that resumes in place re-enters Rust there.
        listener = scope.launch(io) {
            while (true) {
                val event = try {
                    events.next()
                } catch (e: IllegalStateException) {
                    // close() freed the stream between two reads; anything else is a real error.
                    if (stream !== events) break else throw e
                } ?: break
                when (event) {
                    is RuntimeEvent.Changed -> {
                        revision = event.revision
                        _changes.value += 1
                    }
                    is RuntimeEvent.EventsDropped -> reconcile()
                    is RuntimeEvent.PhaseChanged -> if (event.resumed) reconcile()
                    is RuntimeEvent.BackgroundWork -> Unit
                }
            }
        }
    }
}
