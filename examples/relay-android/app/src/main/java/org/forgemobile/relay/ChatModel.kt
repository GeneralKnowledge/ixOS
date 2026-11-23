// The Chats tab's state on Android: plain Kotlin and coroutines (no Android UI), so JVM tests
// drive it against the real Rust core. Like the iPhone app's ChatModel.swift, it holds no chat
// rules: conversations, the store, the outbox and its retries, unread counts and who wrote what
// are decided in Rust (examples/relay/rust/src/chat). This model mirrors what Rust reports,
// through command results and one event stream.
//
// Each trusted device is a conversation (Relay's `use_relay`), with or without a chat server
// account, and its messages go through Relay's one outbox: Chats has no transport of its own.
package org.forgemobile.relay

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.forgemobile.relay.kit.ForgeAlert
import org.forgemobile.relay.kit.ForgeFailure
import uniffi.relay_core.ChatAccount
import uniffi.relay_core.ChatAttachment
import uniffi.relay_core.ChatConversation
import uniffi.relay_core.ChatCore
import uniffi.relay_core.ChatDelivery
import uniffi.relay_core.ChatEvent
import uniffi.relay_core.ChatEvents
import uniffi.relay_core.ChatException
import uniffi.relay_core.ChatMessage
import uniffi.relay_core.ChatMessageStatus
import uniffi.relay_core.ChatOutgoingFile
import uniffi.relay_core.RelayCore
import uniffi.relay_core.chatDelivery

/** Longest message text, in Unicode code points (chat-core's `MAX_BODY_CHARS`). */
const val MAX_MESSAGE_CHARS = 4000

/** Most files one message carries (chat-core's limit, as the iPhone composer says it). */
const val MAX_ATTACHMENTS = 10

/** The state of a conversation's live connection, as the chat screen shows it. */
sealed interface ConnectionState {
    data object Closed : ConnectionState
    data object Connecting : ConnectionState
    data object Connected : ConnectionState
    /** The connection dropped (or the app was in the background); Rust is reconnecting. */
    data class Reconnecting(val reason: String) : ConnectionState
}

/** What Rust says about the conversations now: the Chats screens show this. */
data class ChatState(
    val account: ChatAccount? = null,
    /** Conversations in Rust's order. */
    val conversations: List<ChatConversation> = emptyList(),
    /** Messages per conversation, oldest first, for conversations that have been opened. */
    val messages: Map<String, List<ChatMessage>> = emptyMap(),
    val connection: Map<String, ConnectionState> = emptyMap(),
    val isRefreshing: Boolean = false,
    /** Whether a listing has answered, so an empty list means "none" rather than "still asking". */
    val hasListed: Boolean = false,
    /** Why the last refresh failed (stored conversations stay on screen). */
    val listError: String? = null,
    /** Why the last send or retry failed, per conversation. */
    val sendError: Map<String, String> = emptyMap(),
    /** Why opening a conversation (going live or catching up) failed, per conversation. */
    val roomError: Map<String, String> = emptyMap(),
    /** Set when the server stopped accepting this device's account. */
    val accountProblem: String? = null,
    /** Our messages waiting in Rust's outbox; Rust sends them by itself. */
    val waitingCount: Int = 0,
) {
    fun conversation(id: String): ChatConversation? = conversations.firstOrNull { it.id == id }

    fun title(id: String): String = conversation(id)?.title ?: id

    fun messages(id: String): List<ChatMessage> = messages[id] ?: emptyList()

    fun message(id: String, conversationId: String): ChatMessage? =
        messages[conversationId]?.firstOrNull { it.id == id }

    /** Most recent activity first; conversations without messages keep Rust's order, after. */
    val sortedConversations: List<ChatConversation>
        get() = conversations.withIndex()
            .sortedWith(compareByDescending<IndexedValue<ChatConversation>> { activity(it.value) }.thenBy { it.index })
            .map { it.value }

    /** Conversations muted on this device, for the Chat Server screen. */
    val mutedConversations: List<ChatConversation> get() = conversations.filter { it.muted }

    /** Conversations with the unread dot, for the tab's badge. */
    val unreadConversationCount: Int get() = conversations.count { it.showsUnread }

    private fun activity(conversation: ChatConversation): Long =
        maxOf(conversation.lastMessage?.sentAt ?: Long.MIN_VALUE, conversation.lastMessageAt ?: Long.MIN_VALUE)
}

class ChatModel(
    val core: ChatCore,
    relay: RelayCore,
    /** Where the event stream is read and its events applied; as long-lived as the model. */
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate),
    /** Where Rust's async calls are awaited: a pool, never a dispatcher that resumes in place. */
    private val rust: CoroutineDispatcher = Dispatchers.Default,
) {
    /** Confirmations the screens ask for. */
    sealed interface Action {
        data class Delete(val messageId: String, val conversationId: String) : Action
    }

    private val _state = MutableStateFlow(ChatState())
    val state: StateFlow<ChatState> = _state.asStateFlow()

    private val _alert = MutableStateFlow<ForgeAlert<Action>?>(null)
    val alert: StateFlow<ForgeAlert<Action>?> = _alert.asStateFlow()

    /** Haptics: failure when one of our messages fails, success for someone's message elsewhere. */
    private val _feedback = MutableSharedFlow<Feedback>(extraBufferCapacity = 8)
    val feedback: SharedFlow<Feedback> = _feedback.asSharedFlow()

    private val events: ChatEvents = core.chatEvents()
    private val listening: Job

    init {
        // Subscribe before reading, so nothing is missed in between.
        listening = scope.launch {
            while (true) {
                val event = try {
                    withContext(rust) { events.next() }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    null
                } ?: break
                apply(event)
            }
        }
        showAccount(core.account(), force = true)
        // Each trusted device is a conversation, signed in or not.
        core.useRelay(relay)
        scope.launch { refresh() }
    }

    /** Ends the event stream. The core itself closes with the host. */
    fun close() {
        events.cancel()
        listening.cancel()
    }

    // The chat server account

    /**
     * Creates an account named [displayName] on the chat server at [serverUrl] and signs this
     * device in with it; its rooms join the list. Throws, so the form stays open with what was
     * typed.
     */
    suspend fun signUp(serverUrl: String, displayName: String) {
        val account = withContext(rust) { core.signUp(serverUrl.trim(), displayName.trim()) }
        showAccount(account)
    }

    /** Forgets the account and its rooms on this device. Device conversations stay. */
    fun signOut() = perform {
        core.signOut()
        showAccount(null)
    }

    /** Creates a room on the chat server and returns its id, ready to open. Throws for the form. */
    suspend fun createConversation(title: String): String {
        val created = withContext(rust) { core.createConversation(title.trim()) }
        _state.update { s ->
            if (s.conversation(created.id) != null) s else s.copy(conversations = s.conversations + created)
        }
        return created.id
    }

    // Conversations

    /** Lists the conversations again (pull to refresh, on resume, when trust changes). */
    suspend fun refresh() {
        _state.update { it.copy(isRefreshing = true) }
        try {
            val seen = changes
            val fetched = withContext(rust) { core.listConversations() }
            // An event that came in meanwhile is in the store but maybe not in what came back.
            val list = if (changes == seen) fetched else core.cachedConversations()
            _state.update { it.copy(conversations = list, listError = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(listError = describe(e)) }
        } finally {
            _state.update { it.copy(isRefreshing = false, hasListed = true) }
            updateWaitingCount()
        }
    }

    /** Marks the conversation read here and for its other side (a swipe or the menu). */
    suspend fun markRead(conversationId: String) {
        try {
            withContext(rust) { core.markRead(conversationId) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            fail(e)
        }
    }

    /** Shows the conversation as unread until it is opened. */
    fun markUnread(conversationId: String) = perform { core.setMarkedUnread(conversationId, true) }

    /** Mutes or unmutes the conversation's alerts on this device. */
    fun setMuted(conversationId: String, muted: Boolean) = perform { core.setMuted(conversationId, muted) }

    // A conversation

    /** Where one of our messages is: sending, sent, read or failed. Rust decides. */
    fun delivery(message: ChatMessage): ChatDelivery =
        chatDelivery(message, _state.value.conversation(message.conversationId))

    /** Opens a conversation: shows stored messages, goes live, then catches up. */
    suspend fun enter(conversationId: String) {
        val stored = core.cachedMessages(conversationId)
        _state.update { s ->
            val now = s.connection[conversationId]
            val connecting = if (now is ConnectionState.Connected || now is ConnectionState.Reconnecting) now else ConnectionState.Connecting
            s.copy(
                messages = s.messages + (conversationId to stored),
                connection = s.connection + (conversationId to connecting),
                roomError = s.roomError - conversationId,
            )
        }
        try {
            core.subscribe(conversationId)
        } catch (e: Exception) {
            // No stream started, so no event will move it on from "Connecting…".
            _state.update {
                it.copy(
                    connection = it.connection + (conversationId to ConnectionState.Closed),
                    roomError = it.roomError + (conversationId to describe(e)),
                )
            }
            return
        }
        try {
            val seen = changes
            val fetched = withContext(rust) { core.syncMessages(conversationId) }
            // An event that came in meanwhile is in the store but maybe not in what came back.
            val synced = if (changes == seen) fetched else core.cachedMessages(conversationId)
            _state.update { it.copy(messages = it.messages + (conversationId to synced)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Live, but the catch-up failed: stored messages stay on screen.
            _state.update { it.copy(roomError = it.roomError + (conversationId to describe(e))) }
        }
    }

    /** Leaves a conversation. Messages still being sent finish anyway. */
    fun leave(conversationId: String) {
        runCatching { core.unsubscribe(conversationId) }
    }

    /**
     * Sends [text], as an answer to [replyTo] if given; the bubble appears through Rust's
     * event. Away, it waits in the outbox and goes when the device is reached, even after a
     * relaunch. False when Rust refused it before keeping it (too long), so the screen gives
     * the text back.
     */
    suspend fun send(text: String, conversationId: String, replyTo: String? = null): Boolean {
        val body = text.trim()
        if (body.isEmpty()) return false
        clearSendError(conversationId)
        return try {
            withContext(rust) { core.sendReply(conversationId, body, replyTo) }
            true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(sendError = it.sendError + (conversationId to describe(e))) }
            !refusedBeforeKeeping(e)
        } finally {
            updateWaitingCount()
        }
    }

    /**
     * Sends files (Rust copies them into its store at once) with [text] as an optional caption.
     * False when Rust refused them before keeping them, so the screen keeps them.
     */
    suspend fun send(files: List<ChatOutgoingFile>, text: String, conversationId: String, replyTo: String? = null): Boolean {
        if (files.isEmpty()) return false
        clearSendError(conversationId)
        return try {
            withContext(rust) { core.sendFiles(conversationId, text.trim(), replyTo, files) }
            true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(sendError = it.sendError + (conversationId to describe(e))) }
            !refusedBeforeKeeping(e)
        } finally {
            updateWaitingCount()
        }
    }

    /** Sends a failed (or still waiting) message again now. */
    suspend fun retry(message: ChatMessage) {
        if (message.status == ChatMessageStatus.SENT) return
        clearSendError(message.conversationId)
        try {
            withContext(rust) { core.retryMessage(message.id) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(sendError = it.sendError + (message.conversationId to describe(e))) }
        }
        updateWaitingCount()
    }

    /** The attachment's file on this device, fetching it the first time. Null if that fails. */
    suspend fun localFile(attachment: ChatAttachment): String? {
        core.attachmentFile(attachment.id)?.let { return it }
        return try {
            withContext(rust) { core.fetchAttachment(attachment.id) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }
    }

    /** Replaces the text of one of our sent messages, for everyone. False when it failed. */
    suspend fun edit(message: ChatMessage, text: String): Boolean {
        val body = text.trim()
        if (body.isEmpty() || body == message.body) return true
        return try {
            upsert(withContext(rust) { core.editMessage(message.id, body) })
            true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            fail(e)
            false
        }
    }

    /** Asks before deleting one of our messages. */
    fun askToDelete(message: ChatMessage) {
        val sent = message.status == ChatMessageStatus.SENT
        _alert.value = ForgeAlert.confirm(
            title = if (sent) "Delete this message for everyone?" else "Delete this unsent message?",
            confirm = "Delete",
            action = Action.Delete(message.id, message.conversationId),
            identifier = "chat.confirmDelete",
            destructive = true,
        )
    }

    /** Deletes one of our messages for everyone (a "deleted" placeholder stays); unsent goes. */
    suspend fun delete(messageId: String, conversationId: String) {
        try {
            val tombstone = withContext(rust) { core.deleteMessage(messageId) }
            if (tombstone != null) upsert(tombstone) else remove(messageId, conversationId)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            fail(e)
        }
        updateWaitingCount()
    }

    /** Adds our [emoji] to a message, or takes it away if it is already ours. */
    suspend fun toggleReaction(emoji: String, message: ChatMessage) {
        val mine = message.reactions.any { it.emoji == emoji && it.mine }
        try {
            upsert(withContext(rust) { core.setReaction(message.id, emoji, !mine) })
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            fail(e)
        }
    }

    /** The conversation's unsent composer text, kept by Rust across launches. */
    fun draft(conversationId: String): String = runCatching { core.draft(conversationId) }.getOrNull() ?: ""

    /** Saves the composer's text (empty clears it). Saved on every edit, so no alert. */
    fun saveDraft(text: String, conversationId: String) {
        runCatching { core.setDraft(conversationId, text) }
    }

    /** Messages on this device with every word of [query], newest first (Rust's index); null if the search failed. */
    suspend fun search(query: String, conversationId: String? = null): List<ChatMessage>? {
        if (query.isBlank()) return emptyList()
        return try {
            withContext(rust) { core.search(query, conversationId) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }
    }

    // Alerts

    fun dismissAlert() {
        _alert.value = null
    }

    suspend fun handle(action: Action) {
        _alert.value = null
        when (action) {
            is Action.Delete -> delete(action.messageId, action.conversationId)
        }
    }

    /** Shows a failure from the screens (a file that couldn't be read or opened). */
    fun show(failure: ForgeFailure) {
        _alert.value = ForgeAlert.failure(failure)
    }

    // Events

    /** Applies one Rust event. Internal so tests can drive it directly. */
    /** Events applied so far: a list read across one is read again from the store. */
    private var changes = 0L

    internal fun apply(event: ChatEvent) {
        changes++
        when (event) {
            is ChatEvent.AccountChanged -> showAccount(event.account)
            is ChatEvent.AccountRejected -> _state.update { it.copy(accountProblem = event.reason) }
            is ChatEvent.ConversationUpdated -> _state.update { s ->
                val i = s.conversations.indexOfFirst { it.id == event.conversation.id }
                s.copy(
                    conversations = if (i >= 0) {
                        s.conversations.toMutableList().also { it[i] = event.conversation }
                    } else {
                        s.conversations + event.conversation
                    },
                )
            }
            // Someone else's message in a conversation that isn't open (and isn't muted).
            is ChatEvent.Notify -> _feedback.tryEmit(Feedback.SUCCESS)
            is ChatEvent.MessageReceived -> {
                upsert(event.message)
                if (event.message.isMine) updateWaitingCount()
            }
            // Edits, deletions and reactions: only conversations on screen keep a list.
            is ChatEvent.MessageUpdated ->
                if (_state.value.messages[event.message.conversationId]?.any { it.id == event.message.id } == true) {
                    upsert(event.message)
                }
            is ChatEvent.MessageRemoved -> {
                remove(event.id, event.conversationId)
                updateWaitingCount()
            }
            is ChatEvent.MessageStatusChanged -> {
                updateWaitingCount()
                val list = _state.value.messages[event.conversationId] ?: return
                val index = list.indexOfFirst { it.id == event.id }
                if (index < 0) return
                if (event.status == ChatMessageStatus.FAILED && list[index].isMine) _feedback.tryEmit(Feedback.FAILURE)
                // A sent message carries the other side's time and may move: re-read the store.
                val stored = runCatching { core.cachedMessages(event.conversationId) }.getOrDefault(emptyList())
                val updated = if (stored.any { it.id == event.id }) {
                    stored
                } else {
                    list.toMutableList().also { it[index] = it[index].copy(status = event.status) }
                }
                _state.update { it.copy(messages = it.messages + (event.conversationId to updated)) }
            }
            is ChatEvent.StreamConnected -> setConnection(event.conversationId, ConnectionState.Connected)
            is ChatEvent.StreamDisconnected -> setConnection(
                event.conversationId,
                if (event.willRetry) ConnectionState.Reconnecting(event.reason) else ConnectionState.Closed,
            )
            // Back from the background: Rust has reconnected; fetch what changed meanwhile.
            is ChatEvent.PhaseChanged -> if (event.resumed) scope.launch { refresh() }
            // Fell behind: the store is the truth.
            is ChatEvent.EventsDropped -> reloadFromStore()
        }
    }

    private fun setConnection(conversationId: String, state: ConnectionState) {
        _state.update { it.copy(connection = it.connection + (conversationId to state)) }
    }

    private fun upsert(message: ChatMessage) {
        _state.update { s ->
            val list = (s.messages[message.conversationId] ?: emptyList()).toMutableList()
            val i = list.indexOfFirst { it.id == message.id }
            if (i >= 0) list[i] = message else list.add(message)
            list.sortWith(compareBy<ChatMessage> { it.sentAt }.thenBy { it.id })
            s.copy(messages = s.messages + (message.conversationId to list))
        }
    }

    private fun remove(id: String, conversationId: String) {
        _state.update { s ->
            val list = s.messages[conversationId] ?: return@update s
            s.copy(messages = s.messages + (conversationId to list.filter { it.id != id }))
        }
    }

    private fun reloadFromStore() {
        _state.update { s ->
            s.copy(
                conversations = core.cachedConversations(),
                messages = s.messages.mapValues { (id, _) -> core.cachedMessages(id) },
            )
        }
    }

    /** Switches to [account] (null: devices only), starting fresh when it changed. */
    private fun showAccount(account: ChatAccount?, force: Boolean = false) {
        if (!force && account == _state.value.account) return
        _state.value = ChatState(
            account = account,
            conversations = runCatching { core.cachedConversations() }.getOrDefault(emptyList()),
            waitingCount = runCatching { core.outboxCount().toInt() }.getOrDefault(0),
        )
        if (!force) scope.launch { refresh() }
    }

    private fun updateWaitingCount() {
        val count = runCatching { core.outboxCount().toInt() }.getOrDefault(0)
        _state.update { it.copy(waitingCount = count) }
    }

    private fun clearSendError(conversationId: String) {
        _state.update { it.copy(sendError = it.sendError - conversationId) }
    }

    private fun perform(action: () -> Unit) {
        try {
            action()
        } catch (e: Exception) {
            fail(e)
        }
    }

    private fun fail(error: Throwable) {
        ForgeAlert.failure<Action>(error)?.let { _alert.value = it }
    }

    companion object {
        /** Text for the screen, in Relay's words. */
        fun describe(error: Throwable): String = when (error) {
            is ChatException -> RelayErrors.describeChat(error)
            else -> ForgeFailure.from(error).message
        }

        /**
         * Whether Rust refused a message before keeping it: by its rules (too long, too many
         * files, a reply to a message that is gone), with no account or Relay to send it, with
         * the app closing, or because a picked file couldn't be copied in. Any other failure
         * leaves a bubble that says "Not Sent" and can be retried, as the iPhone app's does.
         */
        fun refusedBeforeKeeping(error: Throwable): Boolean =
            error is ChatException.InvalidInput || error is ChatException.NotSignedIn ||
                error is ChatException.Storage || error is ChatException.ShutDown

        /** The text where only a line fits (a list row, a quote): the body, or "Photo" / a name. */
        fun summary(message: ChatMessage): String {
            if (message.body.isNotEmpty() || message.attachments.isEmpty()) return message.body
            val first = message.attachments[0]
            val what = when {
                first.isImage -> "Photo"
                first.mime.startsWith("audio/") -> "Voice message"
                else -> first.name
            }
            return if (message.attachments.size > 1) "$what +${message.attachments.size - 1}" else what
        }
    }
}
