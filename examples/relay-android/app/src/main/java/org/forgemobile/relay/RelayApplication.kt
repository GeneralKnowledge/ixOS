package org.forgemobile.relay

import android.app.Application
import android.content.Intent
import android.content.pm.ApplicationInfo
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
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.forgemobile.ffi.ForgeAndroid
import org.forgemobile.relay.host.ForgeEnvironment
import org.forgemobile.relay.host.AndroidNotifier
import org.forgemobile.relay.host.ForgeHost
import org.forgemobile.relay.host.ForgeServices
import org.forgemobile.relay.host.NotificationReceiver
import org.forgemobile.relay.kit.ForgeFailure
import uniffi.forge_app.AppPhase
import uniffi.relay_core.deliverNotificationResponse

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

    /** The trusted devices Chats last listed, by id and name. */
    private var listedDevices: Set<Pair<String, String>>? = null

    private val _chat = MutableStateFlow<ChatModel?>(null)

    /** The Chats tab's model, beside [model]; null while closed or when Chats couldn't open. */
    val chat: StateFlow<ChatModel?> = _chat.asStateFlow()

    /** Whether the app is on screen: the Devices screen advertises and looks only then. */
    private val _onScreen = MutableStateFlow(false)
    val onScreen: StateFlow<Boolean> = _onScreen.asStateFlow()

    override fun onCreate() {
        super.onCreate()
        // Android's trust store for Rust's HTTPS (Relay's servers), as on every Forge app.
        ForgeAndroid.init(this)
        // The Keystore, notifications and permissions, before any Rust core opens.
        try {
            ForgeServices.install(this)
        } catch (e: Exception) {
            // Without them Relay still opens: its key stays in `device.key`, and Chats keeps a
            // chat server account only until it closes.
            android.util.Log.w("Relay", "platform services not installed: ${e.javaClass.simpleName}")
        }
        // A Reply or Mark as Read with Relay closed opens its cores in the background first.
        NotificationReceiver.onResponse = {
            withContext(Dispatchers.Main.immediate) {
                // Opens here (a no-op while an open is already under way, which is waited for).
                if (host.core == null) host.start()
                host.status.first { it is ForgeHost.Status.Ready || it is ForgeHost.Status.Failed } is ForgeHost.Status.Ready
            }
        }
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
                val chat = core?.chat?.takeIf { status == ForgeHost.Status.Ready }
                if (_chat.value?.core !== chat) {
                    _chat.value?.close()
                    _chat.value = chat?.let { ChatModel(it, core.core) }
                }
            }
        }
        scope.launch {
            host.changes.collect {
                val model = _model.value ?: return@collect
                model.reload()
                // A device trusted, renamed or revoked: its conversation joins or leaves Chats.
                val now = model.state.value.trustedDevices.map { it.endpointId to it.name }.toSet()
                if (now != listedDevices) {
                    listedDevices = now
                    _chat.value?.let { chat -> launch { chat.refresh() } }
                }
            }
        }
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

    /** Whether [launch] ran in this process. */
    var launched = false
        private set

    /**
     * Opens Relay for a newly launched activity. With [ForgeEnvironment.TEMPORARY_DATA_EXTRA]
     * (UI tests), each launch starts again on a fresh throwaway identity with no servers, as
     * `-ForgeTemporaryData` does on iOS; the person's own data is never opened. [restored] is
     * an activity Android brought back after ending the process: its intent was handled
     * before, so a notification it carries isn't opened again over the restored screens.
     */
    fun launch(intent: Intent, restored: Boolean = false) {
        launched = true
        // Tests only: any app can start the activity, so a release build never lets an extra
        // swap the person's Relay for throwaway data.
        val debuggable = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        val throwaway = debuggable && intent.getBooleanExtra(ForgeEnvironment.TEMPORARY_DATA_EXTRA, false)
        if (throwaway) {
            if (host.core != null) host.close()
            network.close()
            temporary = true
            // A fresh identity each time, never the person's key or account.
            if (ForgeServices.isInstalled) ForgeServices.secureStore.throwaway = java.util.concurrent.ConcurrentHashMap()
        }
        if (!restored) open(intent)
        scope.launch { host.start() }
    }

    private val _link = MutableStateFlow<ChatLink?>(null)

    /** A conversation a notification asked to open, until the screens take it. */
    val link: StateFlow<ChatLink?> = _link.asStateFlow()

    fun takeLink() {
        _link.value = null
    }

    /**
     * Handles an intent that opened the activity: a tap on a message notification opens its
     * conversation, and Rust hears about the tap.
     */
    fun open(intent: Intent) {
        val response = AndroidNotifier.response(intent) ?: return
        // Any app can start the activity: only a tap (or an action that opens the app) comes
        // this way. Reply and Mark as Read come only through the receiver.
        if (!ForgeServices.isInstalled || !ForgeServices.notifier.opensApp(response)) return
        deliverNotificationResponse(response)
        intent.data?.toString()?.let(ChatLink::parse)?.let { _link.value = it }
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
