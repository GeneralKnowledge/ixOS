package org.forgemobile.pocketlibrary

import android.app.Application
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import org.forgemobile.pocketlibrary.ui.PocketLibraryApp
import uniffi.forge_app.AppPhase
import uniffi.pocket_library_core.PocketLibrary

/**
 * Keeps the host and the model across configuration changes (rotation), so the Rust core is
 * opened once per process, as ForgeKit does on iOS.
 */
class AppViewModel(application: Application, saved: SavedStateHandle) : AndroidViewModel(application) {
    private val temporaryData: Boolean = saved[ForgeEnvironment.TEMPORARY_DATA_EXTRA] ?: false

    val host = ForgeHost(
        scope = viewModelScope,
        environment = {
            ForgeEnvironment.standard(
                app = "PocketLibrary",
                files = application.filesDir,
                cache = application.cacheDir,
                temporary = temporaryData,
            )
        },
        open = { env -> LibraryCore(PocketLibrary(env.dataDirectory)) },
    )

    val model = LibraryModel(host, viewModelScope)

    init {
        viewModelScope.launch { host.start() }
    }

    override fun onCleared() {
        host.close()
    }
}

class MainActivity : ComponentActivity() {
    private val app: AppViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // The activity's lifecycle is the app's lifecycle here (one activity): the runtime
        // stops on-screen work in the background and re-reads on return.
        lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onResume(owner: LifecycleOwner) = app.host.report(AppPhase.ACTIVE)
            override fun onPause(owner: LifecycleOwner) = app.host.report(AppPhase.INACTIVE)
            override fun onStop(owner: LifecycleOwner) = app.host.report(AppPhase.BACKGROUND)
        })
        setContent { PocketLibraryApp(app.model) }
    }
}
