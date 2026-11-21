package org.forgemobile.pocketlibrary

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import uniffi.forge_app.AppPhase
import uniffi.pocket_library_core.PocketLibrary

/**
 * Owns the host and the model for the life of the process, as ForgeKit does on iOS: the Rust
 * core opens once, whatever happens to activities (rotation, a second window, the back stack).
 */
class PocketLibraryApplication : Application() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** Set by the first activity, from its [ForgeEnvironment.TEMPORARY_DATA_EXTRA]. */
    private var temporaryData = false

    lateinit var host: ForgeHost<LibraryCore>
        private set
    lateinit var model: LibraryModel
        private set

    override fun onCreate() {
        super.onCreate()
        host = ForgeHost(
            scope = scope,
            environment = {
                ForgeEnvironment.standard(
                    app = "PocketLibrary",
                    files = filesDir,
                    cache = cacheDir,
                    temporary = temporaryData,
                )
            },
            open = { env -> LibraryCore(PocketLibrary(env.dataDirectory)) },
        )
        model = LibraryModel(host, scope)
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
     * Opens the library for a newly launched activity. With [temporary] (UI tests), each launch
     * starts again on an empty throwaway library, as `-ForgeTemporaryData` does on iOS; the
     * person's own library is never opened.
     */
    fun launch(temporary: Boolean) {
        if (temporary) {
            if (host.core != null) host.close()
            temporaryData = true
        }
        model.open()
    }
}
