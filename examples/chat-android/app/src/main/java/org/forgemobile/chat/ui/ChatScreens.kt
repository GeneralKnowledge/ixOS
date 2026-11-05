package org.forgemobile.chat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.forgemobile.chat.ChatSession
import org.forgemobile.chat.ChatViewModel
import org.forgemobile.chat.ConnectionState
import uniffi.forge_ffi.ChatConversation
import uniffi.forge_ffi.ChatMessage
import uniffi.forge_ffi.ChatMessageStatus

/** The whole app: the conversation list, or the open room. */
@Composable
fun ChatRoot(model: ChatViewModel) {
    var openRoom by rememberSaveable { mutableStateOf<String?>(null) }
    var editingServer by rememberSaveable { mutableStateOf(false) }

    MaterialTheme {
        val session = model.session
        val room = openRoom
        when {
            session == null ->
                Unavailable(model.serverUrl, model.openError) { editingServer = true }
            room == null ->
                ConversationList(
                    session = session,
                    serverUrl = model.serverUrl,
                    onRefresh = model::refresh,
                    onOpen = { openRoom = it },
                    onEditServer = { editingServer = true },
                )
            else ->
                ChatRoom(
                    session = session,
                    conversationId = room,
                    onOpen = model::open,
                    onClose = model::close,
                    onSend = model::send,
                    onRetry = model::retry,
                    onBack = { openRoom = null },
                )
        }
        if (editingServer) {
            ServerDialog(
                current = model.serverUrl,
                onDismiss = { editingServer = false },
                onSave = {
                    editingServer = false
                    openRoom = null
                    model.changeServer(it)
                },
            )
        }
    }
}

@Composable
private fun Unavailable(serverUrl: String, error: String?, onEditServer: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("Chat unavailable", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.padding(4.dp))
        Text(error ?: "Opening…", style = MaterialTheme.typography.bodyMedium)
        Text(serverUrl, style = MaterialTheme.typography.bodySmall)
        Spacer(Modifier.padding(8.dp))
        Button(onClick = onEditServer) { Text("Change server") }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ConversationList(
    session: ChatSession,
    serverUrl: String,
    onRefresh: () -> Unit,
    onOpen: (String) -> Unit,
    onEditServer: () -> Unit,
) {
    val state by session.state.collectAsState()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Chats") },
                actions = {
                    TextButton(onClick = onRefresh, enabled = !state.isRefreshing) {
                        Text(if (state.isRefreshing) "Refreshing…" else "Refresh")
                    }
                    TextButton(onClick = onEditServer) { Text("Server") }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.listError?.let { Banner(it, MaterialTheme.colorScheme.errorContainer) }
            if (state.conversations.isEmpty() && state.listError == null) {
                Text("No conversations yet ($serverUrl)", Modifier.padding(16.dp))
            }
            LazyColumn {
                items(state.conversations, key = { it.id }) { conversation ->
                    ConversationRow(conversation) { onOpen(conversation.id) }
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
private fun ConversationRow(conversation: ChatConversation, onClick: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(conversation.title, style = MaterialTheme.typography.titleMedium)
            conversation.lastMessageAt?.let {
                Text(relativeTime(it), style = MaterialTheme.typography.bodySmall)
            }
        }
        if (conversation.unreadCount > 0u) {
            Text(
                text = conversation.unreadCount.toString(),
                color = Color.White,
                modifier =
                    Modifier.background(MaterialTheme.colorScheme.primary, CircleShape)
                        .padding(horizontal = 8.dp, vertical = 2.dp)
                        .semantics { contentDescription = "${conversation.unreadCount} unread" },
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ChatRoom(
    session: ChatSession,
    conversationId: String,
    onOpen: (String) -> Unit,
    onClose: (String) -> Unit,
    onSend: (String, String) -> Unit,
    onRetry: (ChatMessage) -> Unit,
    onBack: () -> Unit,
) {
    val state by session.state.collectAsState()
    var draft by rememberSaveable(conversationId) { mutableStateOf("") }
    val messages = state.messages(conversationId)
    val listState = rememberLazyListState()

    // Entering the screen subscribes; leaving it (back, or a new server) unsubscribes. Sends
    // run in the view model's scope, so they finish either way.
    DisposableEffect(session, conversationId) {
        onOpen(conversationId)
        onDispose { onClose(conversationId) }
    }
    BackHandler(onBack = onBack)
    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size - 1)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(state.title(conversationId)) },
                navigationIcon = { TextButton(onClick = onBack) { Text("Back") } },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().imePadding()) {
            ConnectionBanner(state.connection[conversationId])
            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                items(messages, key = { it.id }) { message ->
                    Bubble(message, isMine = message.author == "me") { onRetry(message) }
                }
            }
            state.sendError[conversationId]?.let {
                Banner(it, MaterialTheme.colorScheme.errorContainer)
            }
            Row(
                modifier = Modifier.fillMaxWidth().padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    placeholder = { Text("Message") },
                    modifier = Modifier.weight(1f),
                )
                TextButton(
                    onClick = {
                        onSend(draft, conversationId)
                        draft = ""
                    },
                    enabled = draft.isNotBlank(),
                ) {
                    Text("Send")
                }
            }
        }
    }
}

@Composable
private fun ConnectionBanner(connection: ConnectionState?) {
    when (connection) {
        ConnectionState.Connecting -> Banner("Connecting…", Color(0xFFE0E0E0))
        is ConnectionState.Reconnecting ->
            Banner(
                "Reconnecting… messages you send now will fail; tap them to retry.",
                Color(0xFFFFE0B2),
            )
        ConnectionState.Closed -> Banner("Not connected", Color(0xFFE0E0E0))
        ConnectionState.Connected, null -> Unit
    }
}

@Composable
private fun Banner(text: String, color: Color) {
    Text(
        text = text,
        style = MaterialTheme.typography.bodySmall,
        modifier = Modifier.fillMaxWidth().background(color).padding(8.dp),
    )
}

@Composable
private fun Bubble(message: ChatMessage, isMine: Boolean, onRetry: () -> Unit) {
    val failed = message.status == ChatMessageStatus.FAILED
    val background =
        when {
            failed -> MaterialTheme.colorScheme.error
            !isMine -> MaterialTheme.colorScheme.surfaceVariant
            message.status == ChatMessageStatus.SENDING -> Color(0xFF9E9E9E)
            else -> MaterialTheme.colorScheme.primary
        }
    val foreground = if (isMine || failed) Color.White else MaterialTheme.colorScheme.onSurface
    Box(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
        contentAlignment = if (isMine) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Column(horizontalAlignment = if (isMine) Alignment.End else Alignment.Start) {
            if (!isMine) {
                Text(message.author, style = MaterialTheme.typography.labelSmall)
            }
            Text(
                text = message.body,
                color = foreground,
                modifier =
                    Modifier.widthIn(max = 280.dp)
                        .background(background, RoundedCornerShape(16.dp))
                        .clickable(enabled = failed, onClick = onRetry)
                        .padding(horizontal = 12.dp, vertical = 8.dp),
            )
            when (message.status) {
                ChatMessageStatus.SENDING ->
                    Text("Sending", style = MaterialTheme.typography.labelSmall)
                ChatMessageStatus.FAILED ->
                    Text(
                        "Not sent. Tap to retry.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                ChatMessageStatus.SENT -> Unit
            }
        }
    }
}

@Composable
private fun ServerDialog(current: String, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var url by rememberSaveable { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Chat server") },
        text = {
            Column {
                OutlinedTextField(value = url, onValueChange = { url = it }, singleLine = true)
                Spacer(Modifier.padding(4.dp))
                Text(
                    "Emulator: http://10.0.2.2:8787. Device: run `adb reverse tcp:8787 " +
                        "tcp:8787` and use http://127.0.0.1:8787, or your computer's LAN " +
                        "address. Android supports http:// servers only.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        },
        confirmButton = { TextButton(onClick = { onSave(url) }) { Text("Save") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

private fun relativeTime(unixMs: Long): String {
    val seconds = (System.currentTimeMillis() - unixMs) / 1000
    return when {
        seconds < 60 -> "just now"
        seconds < 3600 -> "${seconds / 60} min ago"
        seconds < 86_400 -> "${seconds / 3600} h ago"
        else -> "${seconds / 86_400} d ago"
    }
}

