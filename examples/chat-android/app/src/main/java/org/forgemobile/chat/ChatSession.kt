package org.forgemobile.chat

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.forge_ffi.ChatConversation
import uniffi.forge_ffi.ChatEvent
import uniffi.forge_ffi.ChatException
import uniffi.forge_ffi.ChatMessage
import uniffi.forge_ffi.ChatMessageStatus
import uniffi.forge_ffi.ForgeChatAppInterface

/** The state of a room's live connection, as the chat screen shows it. */
sealed interface ConnectionState {
    data object Closed : ConnectionState
    data object Connecting : ConnectionState
    data object Connected : ConnectionState
    /** The connection dropped; Rust is reconnecting with backoff. */
    data class Reconnecting(val reason: String) : ConnectionState
}

/** Everything both screens render. */
data class ChatUiState(
    val conversations: List<ChatConversation> = emptyList(),
    /** Messages per room, oldest first, as last reported by Rust. */
    val messages: Map<String, List<ChatMessage>> = emptyMap(),
    val connection: Map<String, ConnectionState> = emptyMap(),
    val isRefreshing: Boolean = false,
    /** Why the last list refresh failed (cached conversations stay on screen). */
    val listError: String? = null,
    /** Why the last send or retry failed, per room. The bubble shows it too. */
    val sendError: Map<String, String> = emptyMap(),
) {
    fun messages(conversationId: String): List<ChatMessage> = messages[conversationId].orEmpty()

    fun title(conversationId: String): String =
        conversations.firstOrNull { it.id == conversationId }?.title ?: conversationId
}

/**
 * Presentation state for the Chat example, the Kotlin twin of the SwiftUI host's
 * `ChatSession`.
 *
 * Holds no chat logic: the store, sync cursor, reconnects, retries and message ids are all in
 * Rust behind [ForgeChatAppInterface]. This class mirrors what Rust reports through one event
 * stream. Plain Kotlin (no Android types), so JVM tests drive it against the real core.
 *
 * @param scope runs the event loop and owns no commands: commands are suspend functions that
 *   run in the caller's scope. [ChatViewModel] passes `viewModelScope`, so leaving a room never
 *   cancels a send.
 */
class ChatSession(
    val app: ForgeChatAppInterface,
    private val scope: CoroutineScope,
) {
    private val _state = MutableStateFlow(ChatUiState())
    val state: StateFlow<ChatUiState> = _state.asStateFlow()

    private var eventJob: Job? = null

    /** Shows stored conversations at once, starts listening, then refreshes. */
    suspend fun start() {
        if (eventJob == null) {
            _state.update { it.copy(conversations = app.cachedConversations()) }
            listenForEvents()
        }
        refresh()
    }

    /** Re-lists rooms from the server (pull to refresh). */
    suspend fun refresh() {
        _state.update { it.copy(isRefreshing = true) }
        try {
            val list = app.listConversations()
            _state.update { it.copy(conversations = list, listError = null) }
        } catch (e: ChatException) {
            _state.update { it.copy(listError = userMessage(e)) }
        } finally {
            _state.update { it.copy(isRefreshing = false) }
        }
    }

    /** Opens a room: shows stored messages, connects, then catches up. */
    suspend fun open(conversationId: String) {
        _state.update {
            it.copy(
                messages = it.messages + (conversationId to app.cachedMessages(conversationId)),
                connection = it.connection + (conversationId to ConnectionState.Connecting),
            )
        }
        try {
            app.subscribe(conversationId)
            val synced = app.syncMessages(conversationId)
            _state.update { it.copy(messages = it.messages + (conversationId to synced)) }
        } catch (e: ChatException) {
            _state.update { it.copy(sendError = it.sendError + (conversationId to userMessage(e))) }
        }
    }

    /** Leaves a room. Messages still being sent finish anyway. */
    fun close(conversationId: String) {
        try {
            app.unsubscribe(conversationId)
        } catch (_: ChatException) {
            // Shut down already: there is no connection left to close.
        }
    }

    /** Sends [text]. The bubble appears through the `MessageReceived` event. */
    suspend fun send(text: String, conversationId: String) {
        val body = text.trim()
        if (body.isEmpty()) return
        _state.update { it.copy(sendError = it.sendError - conversationId) }
        try {
            app.sendMessage(conversationId, body)
        } catch (e: ChatException) {
            _state.update { it.copy(sendError = it.sendError + (conversationId to userMessage(e))) }
        }
    }

    /** Tapping a failed bubble. */
    suspend fun retry(message: ChatMessage) {
        if (message.status != ChatMessageStatus.FAILED) return
        val room = message.conversationId
        _state.update { it.copy(sendError = it.sendError - room) }
        try {
            app.retryMessage(message.id)
        } catch (e: ChatException) {
            _state.update { it.copy(sendError = it.sendError + (room to userMessage(e))) }
        }
    }

    /** Closes every room and the event stream. The session cannot be used afterwards. */
    fun shutdown() {
        app.shutdown()
        eventJob?.cancel()
    }

    private fun listenForEvents() {
        val events =
            try {
                app.events()
            } catch (e: ChatException) {
                _state.update { it.copy(listError = userMessage(e)) }
                return
            }
        val job =
            scope.launch {
                try {
                    while (true) {
                        val event = events.next() ?: break
                        apply(event)
                    }
                } finally {
                    events.cancel()
                }
            }
        // A coroutine cancelled while suspended in `next()` still ends the Rust subscription.
        job.invokeOnCompletion { events.cancel() }
        eventJob = job
    }

    /** Applies one Rust event. Internal so tests can drive it directly. */
    internal fun apply(event: ChatEvent) {
        when (event) {
            is ChatEvent.ConversationUpdated -> {
                val updated = event.conversation
                _state.update { state ->
                    val list =
                        if (state.conversations.any { it.id == updated.id }) {
                            state.conversations.map { if (it.id == updated.id) updated else it }
                        } else {
                            state.conversations + updated
                        }
                    state.copy(conversations = list)
                }
            }
            is ChatEvent.MessageReceived -> {
                val message = event.message
                _state.update { state ->
                    val room = message.conversationId
                    val list =
                        (state.messages(room).filter { it.id != message.id } + message)
                            .sortedWith(compareBy({ it.sentAt }, { it.id }))
                    state.copy(messages = state.messages + (room to list))
                }
            }
            is ChatEvent.MessageStatusChanged -> {
                val room = event.conversationId
                // Only a room whose messages are loaded needs updating.
                if (_state.value.messages[room]?.any { it.id == event.id } != true) return
                // Re-read from the store: a `SENT` message also carries the server's time and
                // may move.
                val stored = app.cachedMessages(room)
                _state.update { state ->
                    val list =
                        if (stored.any { it.id == event.id }) {
                            stored
                        } else {
                            state.messages(room).map {
                                if (it.id == event.id) it.copy(status = event.status) else it
                            }
                        }
                    state.copy(messages = state.messages + (room to list))
                }
            }
            is ChatEvent.StreamConnected ->
                setConnection(event.conversationId, ConnectionState.Connected)
            is ChatEvent.StreamDisconnected ->
                setConnection(
                    event.conversationId,
                    if (event.willRetry) {
                        ConnectionState.Reconnecting(event.reason)
                    } else {
                        ConnectionState.Closed
                    },
                )
            is ChatEvent.EventsDropped -> {
                // We fell behind: the store is the truth.
                val conversations = app.cachedConversations()
                _state.update { state ->
                    state.copy(
                        conversations = conversations,
                        messages = state.messages.mapValues { (room, _) -> app.cachedMessages(room) },
                    )
                }
            }
        }
    }

    private fun setConnection(conversationId: String, connection: ConnectionState) {
        _state.update { it.copy(connection = it.connection + (conversationId to connection)) }
    }
}
