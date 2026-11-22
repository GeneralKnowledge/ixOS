package org.forgemobile.relay

import android.app.Application
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.forgemobile.ffi.ForgeAndroid
import org.forgemobile.relay.host.ForgeEnvironment
import org.forgemobile.relay.host.ForgeHost
import org.forgemobile.relay.kit.ForgeFailure
import uniffi.forge_app.AppPhase

/**
 * Owns the host, the model and the network for the life of the process, as ForgeKit does on
 * iOS: Relay's Rust core opens once, whatever happens to activities.
 */
class RelayApplication : Application() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** Set by the activity from its launch extras: whether the next open uses throwaway data. */
    private var temporary = false

    lateinit var host: ForgeHost<RelayHostCore>
        private set
    lateinit var network: RelayNetwork
        private set

    private val _model = MutableStateFlow<RelayModel?>(null)

    /** The screens' model, once the core is open; a new one each time it opens. */
    val model: StateFlow<RelayModel?> = _model.asStateFlow()

    /** Whether the app is on screen: the Devices screen advertises and looks only then. */
    private val _onScreen = MutableStateFlow(false)
    val onScreen: StateFlow<Boolean> = _onScreen.asStateFlow()

    override fun onCreate() {
        super.onCreate()
        // Android's trust store for Rust's HTTPS (Relay's servers), as on every Forge app.
        ForgeAndroid.init(this)
        // Relay's errors in its own words, wherever the kit shows a failure (the open too).
        ForgeFailure.describer = RelayErrors
        network = RelayNetwork(this)
        val opener = RelayOpener(::deviceName)
        host = ForgeHost(
            scope = scope,
            environment = {
                ForgeEnvironment.standard(
                    app = "Relay",
                    files = filesDir,
                    cache = cacheDir,
                    temporary = temporary,
                    keep = false,
                )
            },
            // Throwaway data (UI tests) never reaches Relay's real servers.
            open = { env -> opener.open(env, offline = temporary) },
        )
        scope.launch {
            host.status.collect { status ->
                val core = host.core
                _model.value = when {
                    status != ForgeHost.Status.Ready || core == null -> null
                    _model.value?.core === core.core -> _model.value
                    else -> RelayModel(core.core)
                }
            }
        }
        scope.launch { host.changes.collect { _model.value?.reload() } }
        // The process's lifecycle is the app's.
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                host.report(AppPhase.INACTIVE)
                _onScreen.value = true
            }
            override fun onResume(owner: LifecycleOwner) = host.report(AppPhase.ACTIVE)
            override fun onPause(owner: LifecycleOwner) = host.report(AppPhase.INACTIVE)
            override fun onStop(owner: LifecycleOwner) {
                _onScreen.value = false
                host.report(AppPhase.BACKGROUND)
            }
        })
    }

    /**
     * Opens Relay for a newly launched activity. With [ForgeEnvironment.TEMPORARY_DATA_EXTRA]
     * (UI tests), each launch starts again on a fresh throwaway identity with no servers, as
     * `-ForgeTemporaryData` does on iOS; the person's own data is never opened.
     */
    fun launch(intent: Intent) {
        val throwaway = intent.getBooleanExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, false)
        if (throwaway) {
            if (host.core != null) host.close()
            network.close()
            temporary = true
        }
        scope.launch { host.start() }
    }

    /** Starts again after a failed open (Try Again). */
    fun retry() {
        scope.launch { host.start() }
    }

    /** What other devices see this one called at first: the phone's own name, else its model. */
    private fun deviceName(): String =
        Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME)?.takeIf { it.isNotBlank() }
            ?: Build.MODEL
}
