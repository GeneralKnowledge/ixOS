// The Chats tab's list (the iPhone app's ConversationListView.swift): conversations, most recent
// first, with the last message and an unread dot. A long press marks read or unread, or mutes.
// Pull to refresh lists them again. The search field finds messages in every conversation
// stored on this device.
package org.forgemobile.relay.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.forgemobile.relay.ChatModel
import org.forgemobile.relay.ChatText
import uniffi.relay_core.ChatConversation
import uniffi.relay_core.ChatMessage

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatsScreen(model: ChatModel?, problem: String?, open: (Route.Chat) -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    Scaffold(
        topBar = { TopAppBar(title = { Text("Chats") }) },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            if (model == null) {
                // Devices still work; only the conversations couldn't open.
                EmptyState(
                    "Chats Couldn't Open",
                    problem ?: "Close Relay and open it again.",
                    tag = "chats.failure",
                )
                return@Column
            }
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                placeholder = { Text("Search messages") },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                trailingIcon = {
                    if (query.isNotEmpty()) {
                        IconButton(onClick = { query = "" }, modifier = Modifier.testTag("chats.clearSearch")) {
                            Icon(Icons.Filled.Clear, contentDescription = "Clear Search")
                        }
                    }
                },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp)
                    .testTag("chats.search"),
            )
            if (query.isBlank()) {
                ConversationList(model, open)
            } else {
                SearchResults(model, query, open)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ConversationList(model: ChatModel, open: (Route.Chat) -> Unit) {
    val state by model.state.collectAsState()
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    PullToRefreshBox(
        isRefreshing = state.isRefreshing && state.hasListed,
        onRefresh = { scope.launch { model.refresh() } },
        modifier = Modifier.fillMaxSize(),
    ) {
        LazyColumn(Modifier.fillMaxSize().testTag("chats.list")) {
            if (state.waitingCount > 0) {
                item(key = "waiting") { Notice(ChatText.waiting(state.waitingCount), "chats.waiting") }
            }
            val problem = state.accountProblem
            val listError = state.listError
            if (problem != null) {
                item(key = "account") {
                    Notice("The server no longer accepts this account ($problem).", "chats.accountProblem")
                }
            } else if (listError != null) {
                item(key = "listError") { Notice(listError, "chats.listError") }
            }
            items(state.sortedConversations, key = { it.id }) { conversation ->
                ConversationRow(model, conversation) { open(Route.Chat(conversation.id)) }
            }
        }
        if (state.conversations.isEmpty() && state.listError == null) {
            if (state.hasListed) {
                EmptyState(
                    "No Conversations",
                    "Trust a device on the Devices tab, and its conversation appears here.",
                    tag = "chats.empty",
                )
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.testTag("chats.loading"))
                }
            }
        }
    }
}

/** One conversation: unread dot, title, time, and the last message. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ConversationRow(model: ChatModel, conversation: ChatConversation, open: () -> Unit) {
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var menu by remember { mutableStateOf(false) }
    val preview = ChatText.preview(conversation)
    val activity = conversation.lastMessage?.sentAt ?: conversation.lastMessageAt
    Box {
        Row(
            verticalAlignment = Alignment.Top,
            modifier = Modifier
                .fillMaxWidth()
                .testTag("chats.conversation.${conversation.id}")
                .combinedClickable(onClick = open, onLongClick = { menu = true }, onLongClickLabel = "More")
                .semantics(mergeDescendants = true) { contentDescription = ChatText.rowDescription(conversation) }
                .padding(horizontal = 16.dp, vertical = 10.dp),
        ) {
            Box(
                Modifier
                    .padding(top = 6.dp)
                    .size(10.dp)
                    .alpha(if (conversation.showsUnread) 1f else 0f)
                    .background(MaterialTheme.colorScheme.primary, CircleShape),
            )
            Spacer(Modifier.width(8.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        conversation.title,
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (conversation.muted) {
                        Icon(
                            Icons.Filled.Notifications,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(start = 4.dp).size(14.dp),
                        )
                    }
                    Spacer(Modifier.weight(1f))
                    if (activity != null) {
                        Text(
                            ChatText.listTime(activity),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Text(
                    preview,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            if (conversation.showsUnread) {
                DropdownMenuItem(
                    text = { Text("Mark as Read") },
                    onClick = {
                        menu = false
                        scope.launch { model.markRead(conversation.id) }
                    },
                    modifier = Modifier.testTag("chats.markRead"),
                )
            } else {
                DropdownMenuItem(
                    text = { Text("Mark as Unread") },
                    onClick = {
                        menu = false
                        model.markUnread(conversation.id)
                    },
                    modifier = Modifier.testTag("chats.markUnread"),
                )
            }
            DropdownMenuItem(
                text = { Text(if (conversation.muted) "Unmute" else "Mute") },
                onClick = {
                    menu = false
                    model.setMuted(conversation.id, !conversation.muted)
                },
                modifier = Modifier.testTag(if (conversation.muted) "chats.unmute" else "chats.mute"),
            )
        }
    }
}

/** Messages in every conversation that contain each word typed, newest first. */
@Composable
private fun SearchResults(model: ChatModel, query: String, open: (Route.Chat) -> Unit) {
    val state by model.state.collectAsState()
    var results by remember { mutableStateOf<List<ChatMessage>>(emptyList()) }
    // The query the results answer: "No Results" waits for the search of what is typed now.
    var resultsFor by remember { mutableStateOf("") }
    LaunchedEffect(query) {
        delay(200)
        results = model.search(query)
        resultsFor = query
    }
    Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize().testTag("chats.results")) {
            items(results, key = { it.id }) { message ->
                Column(
                    Modifier
                        .fillMaxWidth()
                        .testTag("chats.result")
                        .clickable { open(Route.Chat(message.conversationId, message.id)) }
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                ) {
                    Row {
                        Text(
                            state.title(message.conversationId),
                            style = MaterialTheme.typography.titleSmall,
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            ChatText.listTime(message.sentAt),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Text(
                        highlighted("${if (message.isMine) "You" else message.author}: ${ChatModel.summary(message)}", query),
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        if (results.isEmpty()) {
            if (resultsFor == query) {
                EmptyState("No Results", "Nothing on this device matches “$query”.", tag = "chats.noResults")
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            }
        }
    }
}

/** [text] with each word of [query] in bold where a word starts with it. */
private fun highlighted(text: String, query: String) = buildAnnotatedString {
    val words = query.lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }
    val lower = text.lowercase()
    val bold = BooleanArray(text.length)
    for (word in words) {
        var at = lower.indexOf(word)
        while (at >= 0) {
            if (at == 0 || !lower[at - 1].isLetterOrDigit()) {
                for (i in at until minOf(at + word.length, text.length)) bold[i] = true
            }
            at = lower.indexOf(word, at + 1)
        }
    }
    text.forEachIndexed { i, c ->
        if (bold[i]) withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(c) } else append(c)
    }
}

/** A notice as a row of its own. */
@Composable
internal fun Notice(text: String, tag: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp)
            .background(MaterialTheme.colorScheme.secondaryContainer, MaterialTheme.shapes.medium)
            .padding(12.dp)
            .testTag(tag),
    )
}

@Composable
internal fun EmptyState(title: String, description: String, tag: String) {
    Column(
        Modifier.fillMaxSize().padding(32.dp).testTag(tag),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
        Text(
            description,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp),
        )
    }
}
