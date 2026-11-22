// Relay's Compose screens on Android: the root, its navigation and the dialogs every screen
// shares. Material 3 with ForgeUI's tokens. Test tags use the iPhone app's accessibility
// identifiers (`screen.element`) and are exposed as resource ids, so UI Automator finds them too.
//
// Stage 1 of docs/relay-android.md is the Devices experience; Chats joins it as a second tab in
// stage 2.
package org.forgemobile.relay.ui

import android.view.HapticFeedbackConstants
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.forgemobile.relay.Feedback
import org.forgemobile.relay.PickedFiles
import org.forgemobile.relay.RelayApplication
import org.forgemobile.relay.RelayModel
import org.forgemobile.relay.RelayNetwork
import org.forgemobile.relay.host.ForgeHost
import org.forgemobile.relay.kit.ForgeAlertDialog
import org.forgemobile.relay.kit.ForgeFailure
import org.forgemobile.relay.kit.ForgeLoad
import org.forgemobile.relay.kit.ForgeLoadView
import org.forgemobile.relay.ui.theme.ForgeTheme
import kotlinx.coroutines.flow.StateFlow

/** Where the person is. Kept as text, so it survives rotation. */
sealed interface Route {
    data object Devices : Route
    data class Device(val endpointId: String) : Route
    data object Servers : Route
    data object Send : Route
    data object Receive : Route

    val key: String
        get() = when (this) {
            Devices -> "devices"
            is Device -> "device:$endpointId"
            Servers -> "servers"
            Send -> "send"
            Receive -> "receive"
        }

    companion object {
        fun of(key: String): Route = when {
            key.startsWith("device:") -> Device(key.removePrefix("device:"))
            key == "servers" -> Servers
            key == "send" -> Send
            key == "receive" -> Receive
            else -> Devices
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun RelayApp(app: RelayApplication) {
    ForgeTheme {
        Surface(
            color = MaterialTheme.colorScheme.background,
            modifier = Modifier
                .fillMaxSize()
                .semantics { testTagsAsResourceId = true },
        ) {
            val status by app.host.status.collectAsState()
            val model by app.model.collectAsState()
            val current = model
            when {
                current != null -> RelayScreens(current, app.network, app.onScreen)
                status is ForgeHost.Status.Failed -> {
                    val failure = ForgeFailure(
                        "Can't Open Relay",
                        (status as ForgeHost.Status.Failed).error?.let { ForgeFailure.from(it).message }
                            ?: (status as ForgeHost.Status.Failed).message,
                        isRetryable = true,
                    )
                    ForgeLoadView(
                        ForgeLoad.Failed(failure),
                        onRetry = app::retry,
                        failureTag = "content.failure",
                        retryTag = "content.retry",
                    ) {}
                }
                else -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.testTag("content.loading"))
                }
            }
        }
    }
}

@Composable
private fun RelayScreens(model: RelayModel, network: RelayNetwork, onScreen: StateFlow<Boolean>) {
    val context = LocalContext.current
    val view = LocalView.current
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    // Apart, so finishing a send to a device never removes a code send's copies.
    val codeFiles = remember(model) { PickedFiles(context, "relay-code-send") }
    val deviceFiles = remember(model) { PickedFiles(context, "relay-device-send") }
    var stack by rememberSaveable { mutableStateOf(listOf(Route.Devices.key)) }
    val route = Route.of(stack.last())
    val open: (Route) -> Unit = { stack = stack + it.key }
    val back: () -> Unit = { if (stack.size > 1) stack = stack.dropLast(1) }

    BackHandler(enabled = stack.size > 1, onBack = back)

    // Haptics, whichever screen is showing.
    LaunchedEffect(model) {
        model.feedback.collect { kind ->
            view.performHapticFeedback(
                if (kind == Feedback.SUCCESS) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.REJECT,
            )
        }
    }

    when (route) {
        Route.Devices -> DevicesScreen(model, network, onScreen, open)
        is Route.Device -> TrustedDeviceScreen(model, route.endpointId, deviceFiles, back)
        Route.Servers -> ServersScreen(model, back)
        Route.Send -> SendCodeScreen(model, network, onScreen, codeFiles, back)
        Route.Receive -> ReceiveCodeScreen(model, network, back)
    }

    // Confirmations and failures from any screen.
    val alert by model.alert.collectAsState()
    ForgeAlertDialog(alert, onDismiss = model::dismissAlert) { action ->
        scope.launch { model.handle(action) }
    }
}
