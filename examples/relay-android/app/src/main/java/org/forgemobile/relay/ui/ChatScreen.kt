// One conversation (the iPhone app's ChatView.swift): the transcript, grouped by author and
// time, with an unread divider; the composer with replies, edits, photos and files; and a long
// press on a message for reactions, Reply, Copy, Edit, Try Again and Delete. Its words come from
// ChatText, its state from ChatModel; the rules are Rust's.
package org.forgemobile.relay.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.io.File
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.forgemobile.relay.ChatModel
import org.forgemobile.relay.ChatText
import org.forgemobile.relay.ConnectionState
import org.forgemobile.relay.MAX_ATTACHMENTS
import org.forgemobile.relay.PickedFiles
import org.forgemobile.relay.RelayFormat
import org.forgemobile.relay.kit.ForgeFailure
import uniffi.relay_core.ChatAttachment
import uniffi.relay_core.ChatMessage
import uniffi.relay_core.ChatMessageStatus
import uniffi.relay_core.ChatOutgoingFile

/** A file picked for the next message, copied into the cache. */
private data class Staged(val path: String, val name: String, val mime: String)

/** What the composer is doing besides writing a new message. */
private sealed interface ComposerContext {
    data class Replying(val message: ChatMessage) : ComposerContext
    data class Editing(val message: ChatMessage) : ComposerContext
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(model: ChatModel, conversationId: String, focusMessage: String?, picked: PickedFiles, back: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    val state by model.state.collectAsState()
    val conversation = state.conversation(conversationId)
    val messages = state.messages(conversationId)
    var menu by remember { mutableStateOf(false) }
    var searching by rememberSaveable { mutableStateOf(false) }
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var draft by rememberSaveable(conversationId) { mutableStateOf(model.draft(conversationId)) }
    var composerContext by remember { mutableStateOf<ComposerContext?>(null) }
    val staged = remember { mutableStateListOf<Staged>() }
    // Files still being copied in count against the limit, so two quick picks can't overfill it.
    var copying by remember { mutableIntStateOf(0) }
    var sending by remember { mutableStateOf(false) }
    // How many messages were unread when it opened: the divider sits above them.
    val unreadAtOpen = remember(conversationId) { conversation?.unreadCount?.toInt() ?: 0 }
    var highlighted by remember { mutableStateOf(focusMessage) }

    DisposableEffect(conversationId) {
        scope.launch { model.enter(conversationId) }
        onDispose {
            model.leave(conversationId)
            picked.release()
        }
    }

    fun stage(uris: List<Uri>) {
        if (uris.isEmpty()) return
        val room = MAX_ATTACHMENTS - staged.size - copying
        if (uris.size > room) {
            model.show(ForgeFailure("Too Many Files", "A message can carry up to $MAX_ATTACHMENTS files."))
        }
        val chosen = uris.take(room.coerceAtLeast(0))
        if (chosen.isEmpty()) return
        val mimes = chosen.map { context.contentResolver.getType(it) ?: "application/octet-stream" }
        copying += chosen.size
        scope.launch {
            try {
                val paths = picked.copy(chosen)
                paths.zip(mimes).forEach { (path, mime) -> staged += Staged(path, File(path).name, mime) }
            } catch (e: CancellationException) {
                // The screen closed: its copies go with it (onDispose), and there's nothing to say.
                throw e
            } catch (e: Exception) {
                model.show(ForgeFailure("Couldn't Read the File", "The file couldn't be read. Try picking it again."))
            } finally {
                copying -= chosen.size
            }
        }
    }

    val photos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(MAX_ATTACHMENTS)) { stage(it) }
    val documents = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { stage(it) }

    /** Clears the composer after a send, unless what is in it was written after Send. */
    fun clearAfterSend(text: String, ctx: ComposerContext?) {
        if (draft == text) {
            draft = ""
            model.saveDraft("", conversationId)
        }
        if (composerContext == ctx) composerContext = null
    }

    fun send() {
        if (sending) return
        val text = draft
        val ctx = composerContext
        sending = true
        scope.launch {
            try {
                when {
                    ctx is ComposerContext.Editing -> if (model.edit(ctx.message, text)) {
                        // Back to what was being written before the edit (drafts wait while editing).
                        draft = model.draft(conversationId)
                        composerContext = null
                    }
                    staged.isNotEmpty() -> {
                        // Only what this send carries: a file staged while it runs waits for the next.
                        val sent = staged.toList()
                        val files = sent.map { ChatOutgoingFile(it.path, it.name, it.mime) }
                        val reply = (ctx as? ComposerContext.Replying)?.message?.id
                        if (model.send(files, text, conversationId, reply)) {
                            // Rust copied them in: the cache copies can go.
                            picked.release(sent.map { it.path })
                            staged.removeAll(sent)
                            clearAfterSend(text, ctx)
                        }
                    }
                    else -> {
                        val reply = (ctx as? ComposerContext.Replying)?.message?.id
                        if (model.send(text, conversationId, reply)) clearAfterSend(text, ctx)
                    }
                }
            } finally {
                sending = false
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(conversation?.title ?: state.title(conversationId), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = back, modifier = Modifier.testTag("chat.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    Box {
                        IconButton(onClick = { menu = true }, modifier = Modifier.testTag("chat.more")) {
                            Icon(Icons.Filled.MoreVert, contentDescription = "More")
                        }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(
                                text = { Text("Search in Conversation") },
                                onClick = {
                                    menu = false
                                    searching = true
                                },
                                modifier = Modifier.testTag("chat.search"),
                            )
                            val muted = conversation?.muted == true
                            DropdownMenuItem(
                                text = { Text(if (muted) "Unmute" else "Mute") },
                                onClick = {
                                    menu = false
                                    model.setMuted(conversationId, !muted)
                                },
                                modifier = Modifier.testTag(if (muted) "chat.unmute" else "chat.mute"),
                            )
                        }
                    }
                },
            )
        },
        bottomBar = {
            Composer(
                draft = draft,
                onDraft = {
                    draft = it
                    if (composerContext !is ComposerContext.Editing) model.saveDraft(it, conversationId)
                },
                context = composerContext,
                problem = state.sendError[conversationId],
                staged = staged,
                sending = sending,
                cancelContext = {
                    if (composerContext is ComposerContext.Editing) draft = model.draft(conversationId)
                    composerContext = null
                },
                pickPhotos = { photos.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)) },
                pickDocuments = { documents.launch(arrayOf("*/*")) },
                unstage = { file ->
                    picked.release(listOf(file.path))
                    staged.remove(file)
                },
                send = ::send,
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ConnectionBanner(
                state.connection[conversationId] ?: ConnectionState.Connecting,
                state.roomError[conversationId],
            ) { scope.launch { model.enter(conversationId) } }
            if (searching) {
                ConversationSearch(model, conversationId, searchQuery, { searchQuery = it }, close = {
                    searching = false
                    searchQuery = ""
                }) { found ->
                    searching = false
                    searchQuery = ""
                    highlighted = found
                }
            } else {
                Transcript(
                    model = model,
                    messages = messages,
                    unreadAtOpen = unreadAtOpen,
                    highlighted = highlighted,
                    reply = { composerContext = ComposerContext.Replying(it) },
                    edit = {
                        composerContext = ComposerContext.Editing(it)
                        draft = it.body
                    },
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun ConnectionBanner(state: ConnectionState, problem: String?, reconnect: () -> Unit) {
    val text = when (state) {
        ConnectionState.Connected -> problem
        ConnectionState.Connecting -> "Connecting…"
        is ConnectionState.Reconnecting ->
            "Reconnecting… Messages you send now wait and go out when the connection is back."
        ConnectionState.Closed -> problem ?: "Not connected"
    } ?: return
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .padding(horizontal = 16.dp, vertical = 6.dp)
            .testTag("chat.connection"),
    ) {
        Text(text, style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f))
        if (state == ConnectionState.Closed) {
            TextButton(onClick = reconnect, modifier = Modifier.testTag("chat.reconnect")) { Text("Reconnect") }
        }
    }
}

@Composable
private fun Transcript(
    model: ChatModel,
    messages: List<ChatMessage>,
    unreadAtOpen: Int,
    highlighted: String?,
    reply: (ChatMessage) -> Unit,
    edit: (ChatMessage) -> Unit,
    modifier: Modifier,
) {
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val places = remember(messages) { ChatText.places(messages) }
    // The first unread: the oldest of the last [unreadAtOpen] messages from others.
    val firstUnread = remember(messages.isEmpty(), unreadAtOpen) {
        if (unreadAtOpen <= 0) null else messages.filter { !it.isMine }.takeLast(unreadAtOpen).firstOrNull()?.id
    }
    val atBottom by remember {
        derivedStateOf {
            val last = list.layoutInfo.visibleItemsInfo.lastOrNull()
            last == null || last.index >= list.layoutInfo.totalItemsCount - 1
        }
    }
    var seen by remember { mutableStateOf(messages.size) }
    var opened by remember { mutableStateOf(false) }
    // The highlighted message shown so far: one that isn't loaded yet is shown when it arrives.
    var revealed by remember { mutableStateOf<String?>(null) }
    val target = highlighted?.let { id -> messages.indexOfFirst { it.id == id } }?.takeIf { it >= 0 }

    // Opens at the highlighted message, the first unread, or the end; then follows new messages
    // while at the end, and offers a "New Messages" button otherwise.
    LaunchedEffect(messages.size, highlighted, target) {
        if (messages.isEmpty()) return@LaunchedEffect
        when {
            target != null && revealed != highlighted -> {
                list.scrollToItem(target)
                revealed = highlighted
            }
            !opened -> {
                val unread = firstUnread?.let { id -> messages.indexOfFirst { it.id == id } }?.takeIf { it >= 0 }
                list.scrollToItem(unread ?: messages.lastIndex)
            }
            atBottom || messages.lastOrNull()?.isMine == true -> list.animateScrollToItem(messages.lastIndex)
        }
        opened = true
        if (atBottom) seen = messages.size
    }
    LaunchedEffect(atBottom) { if (atBottom) seen = messages.size }

    Box(modifier.fillMaxWidth()) {
        if (messages.isEmpty()) {
            EmptyState("No Messages Yet", "Write the first one below.", tag = "chat.empty")
        }
        LazyColumn(
            state = list,
            verticalArrangement = Arrangement.spacedBy(2.dp),
            modifier = Modifier.fillMaxSize().padding(horizontal = 12.dp).testTag("chat.transcript"),
        ) {
            itemsIndexed(messages, key = { _, m -> m.id }) { index, message ->
                val place = places[index]
                Column {
                    if (message.id == firstUnread) UnreadDivider(unreadAtOpen)
                    if (place.separator) {
                        Text(
                            ChatText.separator(message.sentAt),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 4.dp),
                            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        )
                    }
                    MessageBubble(
                        model = model,
                        message = message,
                        showsAuthor = place.showsAuthor,
                        showsDelivery = place.showsDelivery,
                        quoted = message.replyTo?.let { id -> messages.firstOrNull { it.id == id } },
                        highlighted = message.id == highlighted,
                        reply = reply,
                        edit = edit,
                    )
                }
            }
        }
        val unseen = messages.size - seen
        if (!atBottom && unseen > 0) {
            SmallFloatingActionButton(
                onClick = { scope.launch { list.animateScrollToItem(messages.lastIndex) } },
                modifier = Modifier.align(Alignment.BottomCenter).padding(8.dp).testTag("chat.newMessages"),
            ) {
                Text(if (unseen == 1) "  1 New Message  " else "  $unseen New Messages  ")
            }
        }
    }
}

@Composable
private fun UnreadDivider(count: Int) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.padding(vertical = 8.dp).testTag("chat.unreadDivider"),
    ) {
        HorizontalDivider(Modifier.weight(1f))
        Text(
            if (count == 1) "  1 new message  " else "  $count new messages  ",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        HorizontalDivider(Modifier.weight(1f))
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageBubble(
    model: ChatModel,
    message: ChatMessage,
    showsAuthor: Boolean,
    showsDelivery: Boolean,
    quoted: ChatMessage?,
    highlighted: Boolean,
    reply: (ChatMessage) -> Unit,
    edit: (ChatMessage) -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var menu by remember { mutableStateOf(false) }
    val mine = message.isMine
    val failed = mine && message.status == ChatMessageStatus.FAILED
    val bubbleColor = when {
        message.deleted -> MaterialTheme.colorScheme.surfaceVariant
        mine -> MaterialTheme.colorScheme.primary
        else -> MaterialTheme.colorScheme.secondaryContainer
    }
    val textColor = when {
        message.deleted -> MaterialTheme.colorScheme.onSurfaceVariant
        mine -> MaterialTheme.colorScheme.onPrimary
        else -> MaterialTheme.colorScheme.onSecondaryContainer
    }
    val open: (ChatAttachment) -> Unit = { attachment ->
        scope.launch {
            val path = model.localFile(attachment)
            if (path == null) {
                model.show(ForgeFailure("Can't Open This", "The file couldn't be fetched. Try again when the other side can be reached."))
            } else {
                ReceivedFiles.open(context, path, model::show, attachment.mime)
            }
        }
    }

    Column(
        horizontalAlignment = if (mine) Alignment.End else Alignment.Start,
        modifier = Modifier.fillMaxWidth().padding(top = if (showsAuthor) 6.dp else 0.dp),
    ) {
        if (showsAuthor) {
            Text(
                message.author,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 12.dp, bottom = 2.dp),
            )
        }
        Box {
            Surface(
                color = bubbleColor,
                shape = RoundedCornerShape(18.dp),
                tonalElevation = if (highlighted) 6.dp else 0.dp,
                border = if (highlighted) androidx.compose.foundation.BorderStroke(2.dp, MaterialTheme.colorScheme.tertiary) else null,
                modifier = Modifier
                    .widthIn(max = 300.dp)
                    .testTag("chat.message.${message.id}")
                    .combinedClickable(
                        onClick = { if (failed) scope.launch { model.retry(message) } },
                        onLongClick = { if (!message.deleted) menu = true },
                        onLongClickLabel = "More",
                    )
                    .semantics(mergeDescendants = true) {
                        contentDescription = buildString {
                            append(if (mine) "You" else message.author)
                            append(": ")
                            append(if (message.deleted) ChatText.DELETED else ChatModel.summary(message))
                            if (message.edited && !message.deleted) append(", edited")
                        }
                    },
            ) {
                Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                    if (message.deleted) {
                        Text(ChatText.DELETED, color = textColor, fontStyle = FontStyle.Italic)
                        return@Column
                    }
                    if (quoted != null) {
                        Text(
                            "${if (quoted.isMine) "You" else quoted.author}: ${ChatModel.summary(quoted)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = textColor.copy(alpha = 0.8f),
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.padding(bottom = 4.dp),
                        )
                    }
                    message.attachments.forEach { attachment ->
                        AttachmentView(model, attachment, textColor) { open(attachment) }
                    }
                    if (message.body.isNotEmpty()) {
                        Text(message.body, color = textColor)
                    }
                    if (message.edited) {
                        Text("Edited", style = MaterialTheme.typography.labelSmall, color = textColor.copy(alpha = 0.7f))
                    }
                }
            }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                if (ChatText.canChange(message)) {
                    Row(Modifier.padding(horizontal = 8.dp)) {
                        ChatText.quickReactions.forEachIndexed { index, emoji ->
                            TextButton(
                                onClick = {
                                    menu = false
                                    scope.launch { model.toggleReaction(emoji, message) }
                                },
                                modifier = Modifier.testTag("chat.react.$index"),
                            ) { Text(emoji) }
                        }
                    }
                }
                DropdownMenuItem(
                    text = { Text("Reply") },
                    onClick = {
                        menu = false
                        reply(message)
                    },
                    modifier = Modifier.testTag("chat.reply"),
                )
                if (message.body.isNotEmpty()) {
                    DropdownMenuItem(
                        text = { Text("Copy") },
                        onClick = {
                            menu = false
                            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            clipboard.setPrimaryClip(ClipData.newPlainText("Relay message", message.body))
                        },
                        modifier = Modifier.testTag("chat.copy"),
                    )
                }
                if (mine && message.status == ChatMessageStatus.SENT && message.body.isNotEmpty() && ChatText.canChange(message)) {
                    DropdownMenuItem(
                        text = { Text("Edit") },
                        onClick = {
                            menu = false
                            edit(message)
                        },
                        modifier = Modifier.testTag("chat.edit"),
                    )
                }
                if (failed) {
                    DropdownMenuItem(
                        text = { Text("Try Again") },
                        onClick = {
                            menu = false
                            scope.launch { model.retry(message) }
                        },
                        modifier = Modifier.testTag("chat.retry"),
                    )
                }
                if (ChatText.canUnsend(message)) {
                    DropdownMenuItem(
                        text = { Text("Delete", color = MaterialTheme.colorScheme.error) },
                        onClick = {
                            menu = false
                            model.askToDelete(message)
                        },
                        modifier = Modifier.testTag("chat.delete"),
                    )
                }
            }
        }
        if (message.reactions.isNotEmpty() && !message.deleted) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp), modifier = Modifier.padding(top = 2.dp)) {
                message.reactions.forEach { reaction ->
                    FilterChip(
                        selected = reaction.mine,
                        onClick = { if (ChatText.canChange(message)) scope.launch { model.toggleReaction(reaction.emoji, message) } },
                        label = { Text("${reaction.emoji} ${reaction.count}") },
                        modifier = Modifier.testTag("chat.reaction"),
                    )
                }
            }
        }
        if (showsDelivery) {
            val delivery = model.delivery(message)
            Text(
                ChatText.delivery(delivery),
                style = MaterialTheme.typography.labelSmall,
                color = if (failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier
                    .padding(horizontal = 8.dp, vertical = 2.dp)
                    .testTag("chat.delivery")
                    .then(if (failed) Modifier.clickable { scope.launch { model.retry(message) } } else Modifier),
            )
        }
    }
}

/** A photo as a thumbnail, any other file as a row with its size. Tapping opens it. */
@Composable
private fun AttachmentView(model: ChatModel, attachment: ChatAttachment, color: androidx.compose.ui.graphics.Color, open: () -> Unit) {
    if (attachment.isImage) {
        // Loading until it settles; null after that means no preview, though Open may still work.
        val preview by produceState<Pair<Boolean, Bitmap?>>(false to null, attachment.id) {
            value = true to model.localFile(attachment)?.let { path -> withContext(Dispatchers.IO) { thumbnail(path) } }
        }
        val (loaded, bitmap) = preview
        Box(
            Modifier
                .padding(bottom = 4.dp)
                .size(200.dp)
                .clickable(onClickLabel = "Open Photo", onClick = open)
                .testTag("chat.attachment"),
            contentAlignment = Alignment.Center,
        ) {
            val image = bitmap
            if (image != null) {
                Image(
                    image.asImageBitmap(),
                    contentDescription = "Photo",
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
            } else if (loaded) {
                Text("Photo", color = color, style = MaterialTheme.typography.bodyMedium)
            } else {
                CircularProgressIndicator(Modifier.size(24.dp))
            }
        }
    } else {
        Column(
            Modifier
                .padding(bottom = 4.dp)
                .clickable(onClickLabel = "Open", onClick = open)
                .testTag("chat.attachment"),
        ) {
            Text(attachment.name, color = color, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(RelayFormat.size(attachment.size), color = color.copy(alpha = 0.7f), style = MaterialTheme.typography.labelSmall)
        }
    }
}

/**
 * A photo scaled down to about [size] pixels on its short side, and to at most [longest] on its
 * long side (a panorama), so a big one never fills memory.
 */
private fun thumbnail(path: String, size: Int = 600, longest: Int = 2048): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(path, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    var sample = 1
    val short = minOf(bounds.outWidth, bounds.outHeight)
    val long = maxOf(bounds.outWidth, bounds.outHeight)
    while (short / (sample * 2) >= size || long / sample > longest) sample *= 2
    return BitmapFactory.decodeFile(path, BitmapFactory.Options().apply { inSampleSize = sample })
}

@Composable
private fun Composer(
    draft: String,
    onDraft: (String) -> Unit,
    context: ComposerContext?,
    problem: String?,
    staged: List<Staged>,
    sending: Boolean,
    cancelContext: () -> Unit,
    pickPhotos: () -> Unit,
    pickDocuments: () -> Unit,
    unstage: (Staged) -> Unit,
    send: () -> Unit,
) {
    var attachMenu by remember { mutableStateOf(false) }
    val editing = context is ComposerContext.Editing
    val canSend = !sending && (draft.isNotBlank() || (staged.isNotEmpty() && !editing))
    Surface(tonalElevation = 3.dp, modifier = Modifier.imePadding()) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 6.dp)) {
            if (context != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val (label, message) = when (context) {
                        is ComposerContext.Replying -> "Replying to ${if (context.message.isMine) "yourself" else context.message.author}" to context.message
                        is ComposerContext.Editing -> "Editing" to context.message
                    }
                    Column(Modifier.weight(1f).padding(start = 8.dp)) {
                        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
                        Text(ChatModel.summary(message), style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    IconButton(onClick = cancelContext, modifier = Modifier.testTag("chat.cancelContext")) {
                        Icon(Icons.Filled.Close, contentDescription = "Cancel")
                    }
                }
            }
            if (staged.isNotEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp), modifier = Modifier.padding(bottom = 4.dp)) {
                    staged.forEach { file ->
                        AssistChip(
                            onClick = { unstage(file) },
                            label = { Text(file.name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            trailingIcon = { Icon(Icons.Filled.Close, contentDescription = "Remove ${file.name}", Modifier.size(16.dp)) },
                            modifier = Modifier.widthIn(max = 160.dp).testTag("chat.staged"),
                        )
                    }
                }
            }
            if (problem != null) {
                Text(
                    problem,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(horizontal = 8.dp).testTag("chat.sendError"),
                )
            }
            ChatText.remaining(draft)?.let { left ->
                Text(
                    "$left characters left",
                    style = MaterialTheme.typography.labelSmall,
                    color = if (left.toInt() < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 8.dp).testTag("chat.tooLong"),
                )
            }
            Row(verticalAlignment = Alignment.Bottom) {
                if (!editing) {
                    Box {
                        IconButton(onClick = { attachMenu = true }, modifier = Modifier.testTag("chat.attach")) {
                            Icon(Icons.Filled.Add, contentDescription = "Attach")
                        }
                        DropdownMenu(expanded = attachMenu, onDismissRequest = { attachMenu = false }) {
                            DropdownMenuItem(
                                text = { Text("Photos") },
                                onClick = {
                                    attachMenu = false
                                    pickPhotos()
                                },
                                modifier = Modifier.testTag("chat.attachPhoto"),
                            )
                            DropdownMenuItem(
                                text = { Text("Files") },
                                onClick = {
                                    attachMenu = false
                                    pickDocuments()
                                },
                                modifier = Modifier.testTag("chat.attachDocument"),
                            )
                        }
                    }
                }
                OutlinedTextField(
                    value = draft,
                    onValueChange = onDraft,
                    placeholder = { Text(if (staged.isEmpty()) "Message" else "Add a caption") },
                    maxLines = 5,
                    modifier = Modifier.weight(1f).heightIn(min = 48.dp).testTag("chat.composer"),
                )
                IconButton(onClick = send, enabled = canSend, modifier = Modifier.testTag("chat.send")) {
                    if (sending) {
                        CircularProgressIndicator(Modifier.size(20.dp))
                    } else {
                        Icon(Icons.AutoMirrored.Filled.Send, contentDescription = if (editing) "Save" else "Send")
                    }
                }
            }
        }
    }
}

/** Searching this conversation's messages on this device; choosing one shows it. */
@Composable
private fun ConversationSearch(
    model: ChatModel,
    conversationId: String,
    query: String,
    onQuery: (String) -> Unit,
    close: () -> Unit,
    found: (String) -> Unit,
) {
    // The results with the query they answer, so rows for an earlier query never show (or open).
    var answer by remember { mutableStateOf<Pair<String, List<ChatMessage>?>?>(null) }
    var attempt by remember { mutableIntStateOf(0) }
    LaunchedEffect(query, attempt) {
        kotlinx.coroutines.delay(200)
        answer = query to model.search(query, conversationId)
    }
    val current = answer?.takeIf { it.first == query }
    val results = current?.second.orEmpty()
    Column(Modifier.fillMaxSize()) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(8.dp)) {
            OutlinedTextField(
                value = query,
                onValueChange = onQuery,
                placeholder = { Text("Search in this conversation") },
                singleLine = true,
                modifier = Modifier.weight(1f).testTag("chat.searchField"),
            )
            TextButton(onClick = close, modifier = Modifier.testTag("chat.searchDone")) { Text("Done") }
        }
        if (current != null && current.second == null) {
            SearchFailed { attempt++ }
            return@Column
        }
        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(results, key = { _, m -> m.id }) { _, message ->
                Column(
                    Modifier
                        .fillMaxWidth()
                        .clickable { found(message.id) }
                        .padding(horizontal = 16.dp, vertical = 8.dp)
                        .testTag("chat.searchResult"),
                ) {
                    Row {
                        Text(if (message.isMine) "You" else message.author, style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f))
                        Text(ChatText.listTime(message.sentAt), style = MaterialTheme.typography.bodySmall)
                    }
                    Text(ChatModel.summary(message), maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
