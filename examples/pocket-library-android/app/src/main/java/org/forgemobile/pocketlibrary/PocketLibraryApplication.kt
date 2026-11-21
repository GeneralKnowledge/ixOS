package org.forgemobile.pocketlibrary

import android.app.Application
import android.content.Intent
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.forgemobile.pocketlibrary.host.ForgeHttpTransport
import org.forgemobile.pocketlibrary.host.WorkManagerBackgroundTime
import uniffi.forge_app.AppPhase

/**
 * Owns the host and the model for the life of the process, as ForgeKit does on iOS: the Rust
 * core opens once, whatever happens to activities (rotation, a second window, the back stack).
 */
class PocketLibraryApplication : Application() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** Set by the activity from its launch extras: how the next open finds its data. */
    private var testData = TestData.NONE

    private val opener = LibraryOpener()

    /** Whether an activity has launched in this process (and so chose the library). */
    private var launched = false

    lateinit var host: ForgeHost<LibraryCore>
        private set
    lateinit var model: LibraryModel
        private set
    lateinit var imports: EpubImports
        private set

    override fun onCreate() {
        super.onCreate()
        // Rust's requests (the catalogue) go through Android's own HTTP stack, as they go
        // through URLSession on iOS.
        ForgeHttpTransport.install()
        host = ForgeHost(
            scope = scope,
            environment = {
                val data = testData
                // Once per launch: a later Try Again or Start Fresh keeps what is there.
                testData = data.reopened()
                ForgeEnvironment.standard(
                    app = "PocketLibrary",
                    files = filesDir,
                    cache = cacheDir,
                    temporary = data.temporary,
                    keep = data.keep,
                ).also { env ->
                    // The test library starts as a file that isn't a library, so a UI test
                    // reaches Start Fresh (iOS: -PocketLibraryUnreadableTestData).
                    if (data.unreadable) File(env.dataDirectory, "library.json").writeText("not a library")
                }
            },
            open = opener::open,
        )
        // Imports, exports and saves finish after the app leaves the screen.
        host.backgroundTime = WorkManagerBackgroundTime(this)
        model = LibraryModel(host, scope, opener)
        imports = EpubImports(this)
        // The process's lifecycle is the app's: the runtime stops on-screen work when every
        // activity has left the screen, and re-reads on return.
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) = host.report(AppPhase.INACTIVE)
            override fun onResume(owner: LifecycleOwner) = host.report(AppPhase.ACTIVE)
            override fun onPause(owner: LifecycleOwner) = host.report(AppPhase.INACTIVE)
            override fun onStop(owner: LifecycleOwner) = host.report(AppPhase.BACKGROUND)
        })
    }

    /**
     * Opens the library for a newly launched activity. With temporary [data] (UI tests), each
     * launch starts again on an empty throwaway library, as `-ForgeTemporaryData` does on iOS,
     * unless it keeps the last launch's; the person's own library is never opened.
     */
    fun launch(data: TestData) {
        launched = true
        imports.runHold = data.importHoldSeconds * 1000L
        if (data.temporary) {
            // A new launch in the same process: open the test library again, as a relaunch.
            if (host.core != null) host.close()
            testData = data
            model.resetView()
        }
        model.open()
        if (data.temporary && data.sampleBook) {
            scope.launch { imports.addSampleBook() }
        }
    }

    /**
     * For a WorkManager job, which may run in a process no activity has started (it was ended
     * with the job still to do): opens the library the job belongs to, [temporary] test data or
     * the person's own, unless an activity has opened one. True once a library is open.
     */
    suspend fun openForWork(temporary: Boolean): Boolean {
        withContext(Dispatchers.Main.immediate) {
            if (host.core != null) return@withContext
            // Keeps the test library the job came from, as a relaunch with keepTestData does.
            if (!launched) testData = if (temporary) TestData(temporary = true, keep = true) else TestData.NONE
            host.start()
        }
        val status = withTimeoutOrNull(30_000) {
            host.status.first { it == ForgeHost.Status.Ready || it is ForgeHost.Status.Failed }
        }
        return status == ForgeHost.Status.Ready && host.core != null
    }

    /** The launch extras UI tests pass, as iOS's launch arguments. */
    data class TestData(
        /** [ForgeEnvironment.TEMPORARY_DATA_EXTRA]: a throwaway library, emptied at launch. */
        val temporary: Boolean,
        /** [ForgeEnvironment.KEEP_TEST_DATA_EXTRA]: keep the last launch's throwaway library. */
        val keep: Boolean = false,
        /** [UNREADABLE_TEST_DATA_EXTRA]: the throwaway library starts unreadable. */
        val unreadable: Boolean = false,
        /** [SAMPLE_BOOK_EXTRA]: an empty throwaway library starts with a sample EPUB in it. */
        val sampleBook: Boolean = false,
        /** [IMPORT_HOLD_EXTRA]: seconds each import waits before it starts, in this process. */
        val importHoldSeconds: Int = 0,
    ) {
        fun reopened() = if (temporary) copy(keep = true, unreadable = false) else this

        companion object {
            val NONE = TestData(temporary = false)

            fun from(intent: Intent): TestData {
                val temporary = intent.getBooleanExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, false)
                if (!temporary) return NONE
                val keep = intent.getBooleanExtra(ForgeEnvironment.KEEP_TEST_DATA_EXTRA, false)
                return TestData(
                    temporary = true,
                    keep = keep,
                    unreadable = !keep && intent.getBooleanExtra(UNREADABLE_TEST_DATA_EXTRA, false),
                    sampleBook = intent.getBooleanExtra(SAMPLE_BOOK_EXTRA, false),
                    importHoldSeconds = intent.getIntExtra(IMPORT_HOLD_EXTRA, 0).coerceIn(0, 120),
                )
            }
        }
    }

    companion object {
        /** iOS's `-PocketLibraryUnreadableTestData`. */
        const val UNREADABLE_TEST_DATA_EXTRA = "pocketLibrary.unreadableTestData"

        /** iOS's `-PocketLibrarySampleBook`: a book to open in the reader. */
        const val SAMPLE_BOOK_EXTRA = "pocketLibrary.sampleBook"

        /**
         * Test data only: imports wait this many seconds before they start, so a test can end
         * the process while one is still to do (Android only; iOS has no such test).
         */
        const val IMPORT_HOLD_EXTRA = "pocketLibrary.holdImportSeconds"
    }
}
