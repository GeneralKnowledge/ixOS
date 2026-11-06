package org.forgemobile.chat

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import kotlinx.coroutines.launch
import uniffi.forge_ffi.ChatAppConfig
import uniffi.forge_ffi.ChatException
import uniffi.forge_ffi.ChatMessage
import uniffi.forge_ffi.ForgeChatApp

/**
 * Owns the Rust app instance for the chosen server and the [ChatSession] over it.
 *
 * Commands run in `viewModelScope`, not in a screen's scope, so leaving a room (or rotating the
 * screen) never cancels a send. Changing the server shuts the old instance down.
 */
class ChatViewModel(application: Application) : AndroidViewModel(application) {
    private val prefs = application.getSharedPreferences("chat", Context.MODE_PRIVATE)
    /** New messages in rooms you are not in show as notifications while the app runs. */
    private val notifier = AndroidChatNotifier(application)

    var serverUrl by mutableStateOf(prefs.getString(KEY_SERVER, null) ?: DEFAULT_SERVER)
        private set
    var session by mutableStateOf<ChatSession?>(null)
        private set
    /** Why the app could not be opened for [serverUrl] (bad URL, https on Android, ...). */
    var openError by mutableStateOf<String?>(null)
        private set

    init {
        connect()
    }

    fun changeServer(url: String) {
        val trimmed = url.trim()
        if (trimmed.isEmpty() || trimmed == serverUrl && session != null) return
        prefs.edit().putString(KEY_SERVER, trimmed).apply()
        serverUrl = trimmed
        connect()
    }

    fun refresh() = launch { it.refresh() }

    fun open(conversationId: String) = launch { it.open(conversationId) }

    fun close(conversationId: String) {
        session?.close(conversationId)
    }

    fun send(text: String, conversationId: String) = launch { it.send(text, conversationId) }

    fun retry(message: ChatMessage) = launch { it.retry(message) }

    private fun launch(command: suspend (ChatSession) -> Unit) {
        val current = session ?: return
        viewModelScope.launch { command(current) }
    }

    private fun connect() {
        session?.shutdown()
        session = null
        openError = null
        val app =
            try {
                ForgeChatApp(
                    storageDirectory(getApplication(), serverUrl),
                    ChatAppConfig(baseUrl = serverUrl, user = null, requestTimeoutMs = null),
                )
            } catch (e: ChatException) {
                openError = userMessage(e)
                return
            }
        val opened = ChatSession(app, viewModelScope, notifier)
        session = opened
        viewModelScope.launch { opened.start() }
    }

    override fun onCleared() {
        session?.shutdown()
        session = null
        super.onCleared()
    }

    companion object {
        private const val KEY_SERVER = "serverURL"

        /** The development machine as the Android emulator sees it. */
        const val DEFAULT_SERVER = "http://10.0.2.2:8787"

        /** One store per server, so switching servers never mixes rooms. */
        fun storageDirectory(context: Context, serverUrl: String): String {
            val safe = serverUrl.map { if (it.isLetterOrDigit()) it else '_' }.joinToString("")
            return File(context.filesDir, "chat/$safe").absolutePath
        }
    }
}
