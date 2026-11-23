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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
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

    /** Asks [authenticate] once (a second call while it asks does nothing). */
    suspend fun unlock(authenticate: suspend () -> AuthenticationResult) {
        synchronized(this) {
            if (_authenticating.value || !_locked.value) return
            _authenticating.value = true
        }
        val result = try {
            authenticate()
        } finally {
            _authenticating.value = false
        }
        synchronized(this) {
            when (result) {
                // No screen lock at all any more: a lock that can never open helps nobody.
                AuthenticationResult.AUTHENTICATED, AuthenticationResult.UNAVAILABLE -> {
                    _locked.value = false
                    _message.value = null
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
 * screen and screenshots (`FLAG_SECURE`), as iOS hides the app in the app switcher. The lock
 * screen's button is `appLock.unlock`.
 */
@Composable
fun ForgeAppLock(
    state: ForgeAppLockState,
    enabled: Boolean,
    authenticate: suspend () -> AuthenticationResult,
    content: @Composable () -> Unit,
) {
    val locked by state.locked.collectAsState()
    val message by state.message.collectAsState()
    val authenticating by state.authenticating.collectAsState()
    val showing = enabled && locked
    val activity = LocalContext.current.activity()
    val scope = rememberCoroutineScope()

    DisposableEffect(enabled, activity) {
        val window = activity?.window
        if (enabled) window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE) else window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { }
    }
    LaunchedEffect(showing) {
        if (showing) state.unlock(authenticate)
    }

    Box(Modifier.fillMaxSize()) {
        // Hidden from TalkBack too while locked.
        Box(if (showing) Modifier.fillMaxSize().clearAndSetSemantics { } else Modifier.fillMaxSize()) { content() }
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
                        onClick = { scope.launch { state.unlock(authenticate) } },
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
