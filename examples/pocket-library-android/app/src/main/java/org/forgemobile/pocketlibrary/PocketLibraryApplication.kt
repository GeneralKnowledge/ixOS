package org.forgemobile.pocketlibrary

import android.app.Application
import android.content.Intent
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import java.io.File
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.forgemobile.pocketlibrary.host.ForgeHttpTransport
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

    lateinit var host: ForgeHost<LibraryCore>
        private set
    lateinit var model: LibraryModel
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
        model = LibraryModel(host, scope, opener)
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
        if (data.temporary) {
            // A new launch in the same process: open the test library again, as a relaunch.
            if (host.core != null) host.close()
            testData = data
            model.resetView()
        }
        model.open()
    }

    /** The launch extras UI tests pass, as iOS's launch arguments. */
    data class TestData(
        /** [ForgeEnvironment.TEMPORARY_DATA_EXTRA]: a throwaway library, emptied at launch. */
        val temporary: Boolean,
        /** [ForgeEnvironment.KEEP_TEST_DATA_EXTRA]: keep the last launch's throwaway library. */
        val keep: Boolean = false,
        /** [UNREADABLE_TEST_DATA_EXTRA]: the throwaway library starts unreadable. */
        val unreadable: Boolean = false,
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
                )
            }
        }
    }

    companion object {
        /** iOS's `-PocketLibraryUnreadableTestData`. */
        const val UNREADABLE_TEST_DATA_EXTRA = "pocketLibrary.unreadableTestData"
    }
}
