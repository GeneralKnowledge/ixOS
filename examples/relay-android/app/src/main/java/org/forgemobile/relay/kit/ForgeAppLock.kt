// The Android form of forge-services' `.forgeAppLock` (ForgeServices+Biometrics.swift): locks the
// app behind a fingerprint, face or the screen lock while the setting is on, at launch and when
// it comes back after more than a minute away. Lives in this app until a second Android app
// needs it, like the rest of kit/ (docs/architecture.md#framework-principles).
package org.forgemobile.relay.kit

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.SystemClock
import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.relay_core.AuthenticationResult

/**
 * Whether the app is locked, for the life of the process, so a rotation doesn't lock it again.
 * The app reports when it leaves the screen and comes back ([left], [returned]); the lock shows
 * only while the setting is on. Plain Kotlin, so JVM tests drive it with their own [clock].
 */
class ForgeAppLockState(
    private val graceMillis: Long = 60_000,
    private val clock: () -> Long = SystemClock::elapsedRealtime,
) {
    private val _locked = MutableStateFlow(true)

    /** Starts locked: a launch asks first, when the setting is on. */
    val locked: StateFlow<Boolean> = _locked.asStateFlow()

    private val _message = MutableStateFlow<String?>(null)
    val message: StateFlow<String?> = _message.asStateFlow()

    private val _authenticating = MutableStateFlow(false)
    val authenticating: StateFlow<Boolean> = _authenticating.asStateFlow()

    private var leftAt: Long? = null

    /** The app left the screen. A prompt on screen doesn't count: it is the unlock itself. */
    @Synchronized
    fun left() {
        if (!_locked.value && !_authenticating.value) leftAt = clock()
    }

    /** The app is back: locked again after more than the grace period away. */
    @Synchronized
    fun returned() {
        val away = leftAt?.let { clock() - it }
        leftAt = null
        if (away != null && away > graceMillis) _locked.value = true
    }

    /** The person just turned the lock on, or off: neither locks the open app. */
    @Synchronized
    fun settingChanged() {
        _locked.value = false
        _message.value = null
        leftAt = null
    }

    /**
     * Asks [authenticate] once (a second call while it asks does nothing). [canLock] says
     * whether the phone has any way to unlock at all, asked again when the answer is
     * `UNAVAILABLE`: only a phone with none opens without one.
     */
    suspend fun unlock(canLock: () -> Boolean, authenticate: suspend () -> AuthenticationResult) {
        synchronized(this) {
            if (_authenticating.value || !_locked.value) return
            _authenticating.value = true
        }
        val result = try {
            authenticate()
        } finally {
            _authenticating.value = false
        }
        // No screen lock at all any more: a lock that can never open helps nobody. A sensor
        // that is only busy or missing, on a phone that still has a screen lock, stays locked.
        val nothingToAsk = result == AuthenticationResult.UNAVAILABLE && !canLock()
        synchronized(this) {
            when (result) {
                AuthenticationResult.AUTHENTICATED -> {
                    _locked.value = false
                    _message.value = null
                }
                AuthenticationResult.UNAVAILABLE -> if (nothingToAsk) {
                    _locked.value = false
                    _message.value = null
                } else {
                    _message.value = "Can't ask right now. Try again in a moment."
                }
                AuthenticationResult.LOCKED_OUT -> _message.value = "Too many attempts. Try again later, or use your screen lock."
                AuthenticationResult.CANCELLED, AuthenticationResult.FAILED -> _message.value = null
            }
        }
    }
}

/**
 * Covers [content] with a lock screen while [enabled] and [state] is locked, and asks
 * [authenticate] at once. While [enabled], the window is also kept out of the recent apps
 * screen and screenshots (`FLAG_SECURE`), as iOS hides the app in the app switcher. While
 * locked, [content] leaves the composition, so a dialog or menu it had open (each in its own
 * window, which the lock screen can't cover) closes with it; what it keeps with
 * `rememberSaveable` comes back on unlock. [canLock] is whether the phone can unlock at all
 * (see [ForgeAppLockState.unlock]). The lock screen's button is `appLock.unlock`.
 */
@Composable
fun ForgeAppLock(
    state: ForgeAppLockState,
    enabled: Boolean,
    canLock: () -> Boolean,
    authenticate: suspend () -> AuthenticationResult,
    content: @Composable () -> Unit,
) {
    val locked by state.locked.collectAsState()
    val message by state.message.collectAsState()
    val authenticating by state.authenticating.collectAsState()
    val showing = enabled && locked
    val activity = LocalContext.current.activity()
    val scope = rememberCoroutineScope()
    val saved = rememberSaveableStateHolder()

    DisposableEffect(enabled, activity) {
        val window = activity?.window
        if (enabled) window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE) else window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { }
    }
    LaunchedEffect(showing) {
        if (showing) state.unlock(canLock, authenticate)
    }

    Box(Modifier.fillMaxSize()) {
        // Not composed at all while locked: an overlay can't cover a dialog's own window.
        if (!showing) saved.SaveableStateProvider("content") { content() }
        if (showing) {
            // Back leaves the app rather than reaching the screen underneath.
            BackHandler { activity?.moveTaskToBack(true) }
            Surface(
                modifier = Modifier
                    .fillMaxSize()
                    .testTag("appLock")
                    // Touches stop here, never reaching the content.
                    .pointerInput(Unit) {
                        awaitPointerEventScope {
                            while (true) awaitPointerEvent().changes.forEach { it.consume() }
                        }
                    },
            ) {
                Column(
                    modifier = Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Icon(Icons.Filled.Lock, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text("Locked", style = MaterialTheme.typography.titleLarge)
                    message?.let {
                        Text(it, style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center)
                    }
                    Button(
                        onClick = { scope.launch { state.unlock(canLock, authenticate) } },
                        enabled = !authenticating,
                        modifier = Modifier.testTag("appLock.unlock"),
                    ) { Text("Unlock") }
                }
            }
        }
    }
}

private tailrec fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}
