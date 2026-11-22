// Relay's Compose screens on Android: the root, its navigation and the dialogs every screen
// shares. Material 3 with ForgeUI's tokens. Test tags use the iPhone app's accessibility
// identifiers (`screen.element`) and are exposed as resource ids, so UI Automator finds them too.
//
// Two tabs, as on the iPhone: Chats (stage 2 of docs/relay-android.md) and Devices (stage 1).
// Each keeps its own stack of screens; the tab bar shows on each tab's first screen.
package org.forgemobile.relay.ui

import android.view.HapticFeedbackConstants
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.DisposableEffect
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
import org.forgemobile.relay.ChatLink
import org.forgemobile.relay.ChatModel
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
import uniffi.relay_core.deviceConversationId

/** Where the person is. Kept as text, so it survives rotation. */
sealed interface Route {
    data object Chats : Route
    /** A conversation, at [messageId] when opened from a search result. */
    data class Chat(val conversationId: String, val messageId: String? = null) : Route
    data object ChatServer : Route
    data object Devices : Route
    data class Device(val endpointId: String) : Route
    data object Servers : Route
    data object Send : Route
    data object Receive : Route

    val key: String
        get() = when (this) {
            Chats -> "chats"
            ChatServer -> "chatServer"
            is Chat -> listOfNotNull("chat", conversationId, messageId).joinToString("\n")
            Devices -> "devices"
            is Device -> "device:$endpointId"
            Servers -> "servers"
            Send -> "send"
            Receive -> "receive"
        }

    companion object {
        fun of(key: String): Route = when {
            key == "chats" -> Chats
            key == "chatServer" -> ChatServer
            key.startsWith("chat\n") -> key.split("\n").let { Chat(it[1], it.getOrNull(2)) }
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
                current != null -> {
                    val chat by app.chat.collectAsState()
                    val link by app.link.collectAsState()
                    RelayScreens(current, chat, app.host.core?.chatProblem, app.network, app.onScreen, link, app::takeLink)
                }
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

/** The two tabs. */
private enum class Tab(val root: Route, val tag: String) {
    CHATS(Route.Chats, "tab.chats"),
    DEVICES(Route.Devices, "tab.devices"),
}

@Composable
private fun RelayScreens(
    model: RelayModel,
    chat: ChatModel?,
    chatProblem: String?,
    network: RelayNetwork,
    onScreen: StateFlow<Boolean>,
    /** A conversation a notification opened, until it is shown. */
    link: ChatLink?,
    tookLink: () -> Unit,
) {
    val context = LocalContext.current
    val view = LocalView.current
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    // Apart, so finishing a send to a device never removes a code send's copies.
    val codeFiles = remember(model) { PickedFiles(context, "relay-code-send") }
    val deviceFiles = remember(model) { PickedFiles(context, "relay-device-send") }
    val chatFiles = remember(model) { PickedFiles(context, "relay-chat-send") }
    // Devices first while there are none to talk to, as the iPhone app opens on Chats.
    var tab by rememberSaveable { mutableStateOf(Tab.CHATS) }
    var chatsStack by rememberSaveable { mutableStateOf(listOf(Route.Chats.key)) }
    var devicesStack by rememberSaveable { mutableStateOf(listOf(Route.Devices.key)) }
    val stack = if (tab == Tab.CHATS) chatsStack else devicesStack
    val setStack: (List<String>) -> Unit = { if (tab == Tab.CHATS) chatsStack = it else devicesStack = it }
    val route = Route.of(stack.last())
    val open: (Route) -> Unit = { setStack(stack + it.key) }
    val back: () -> Unit = { if (stack.size > 1) setStack(stack.dropLast(1)) }

    BackHandler(enabled = stack.size > 1, onBack = back)

    // Open to trusted devices while Relay is on screen, whichever tab: advertise, and look for
    // them. Chats needs it as much as Devices, since a trusted device's local address comes
    // from here. Android stops neither in the background by itself, so leaving withdraws both.
    val visible by onScreen.collectAsState()
    DisposableEffect(model, visible) {
        val job = if (visible) {
            scope.launch {
                network.watchDevices(model::devicesNearby)
                network.advertiseDevice(model.deviceService())
            }
        } else {
            null
        }
        onDispose {
            job?.cancel()
            network.advertiseDevice(null)
            network.stopWatchingDevices()
        }
    }

    // A tap on a message notification: its conversation, at that message.
    LaunchedEffect(link, chat) {
        if (link == null || chat == null) return@LaunchedEffect
        chatsStack = listOf(Route.Chats.key, Route.Chat(link.conversationId, link.messageId).key)
        tab = Tab.CHATS
        tookLink()
    }

    // Haptics, whichever screen is showing.
    LaunchedEffect(model) {
        model.feedback.collect { kind -> view.haptic(kind) }
    }
    LaunchedEffect(chat) {
        chat?.feedback?.collect { kind -> view.haptic(kind) }
    }

    val unread = chat?.state?.collectAsState()?.value?.unreadConversationCount ?: 0
    Scaffold(
        bottomBar = {
            if (stack.size == 1) {
                NavigationBar {
                    Tab.entries.forEach { each ->
                        NavigationBarItem(
                            selected = tab == each,
                            onClick = { tab = each },
                            icon = {
                                if (each == Tab.CHATS) {
                                    BadgedBox(badge = { if (unread > 0) Badge { Text("$unread") } }) {
                                        Icon(Icons.Filled.Email, contentDescription = null)
                                    }
                                } else {
                                    Icon(Icons.Filled.Share, contentDescription = null)
                                }
                            },
                            label = { Text(if (each == Tab.CHATS) "Chats" else "Devices") },
                            modifier = Modifier.testTag(each.tag),
                        )
                    }
                }
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding)) {
            when (route) {
                Route.Chats -> ChatsScreen(chat, chatProblem, open)
                is Route.Chat -> if (chat != null) {
                    ChatScreen(chat, route.conversationId, route.messageId, chatFiles, back)
                } else {
                    // Chats couldn't open: its list says why.
                    LaunchedEffect(Unit) { chatsStack = listOf(Route.Chats.key) }
                }
                Route.ChatServer -> if (chat != null) {
                    ChatServerScreen(chat, back)
                } else {
                    LaunchedEffect(Unit) { chatsStack = listOf(Route.Chats.key) }
                }
                Route.Devices -> DevicesScreen(model, open)
                is Route.Device -> TrustedDeviceScreen(model, route.endpointId, deviceFiles, back) {
                    // A device's Message button opens its conversation in Chats.
                    chatsStack = listOf(Route.Chats.key, Route.Chat(deviceConversationId(route.endpointId)).key)
                    tab = Tab.CHATS
                }
                Route.Servers -> ServersScreen(model, back)
                Route.Send -> SendCodeScreen(model, network, onScreen, codeFiles, back)
                Route.Receive -> ReceiveCodeScreen(model, network, back)
            }
        }
    }

    // Confirmations and failures from any screen.
    val alert by model.alert.collectAsState()
    ForgeAlertDialog(alert, onDismiss = model::dismissAlert) { action ->
        scope.launch { model.handle(action) }
    }
    if (chat != null) {
        val chatAlert by chat.alert.collectAsState()
        ForgeAlertDialog(chatAlert, onDismiss = chat::dismissAlert) { action ->
            scope.launch { chat.handle(action) }
        }
    }
}

private fun android.view.View.haptic(kind: Feedback) {
    performHapticFeedback(
        if (kind == Feedback.SUCCESS) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.REJECT,
    )
}
