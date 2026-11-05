package org.forgemobile.chat

import kotlinx.coroutines.CancellationException
import uniffi.forge_ffi.ChatException

/**
 * Short text for the UI. The exception's own fields (`reason`, `detail`) are diagnostics from
 * Rust, not copy.
 */
fun userMessage(error: Throwable): String =
    when (error) {
        is ChatException.Network -> "Can't reach the chat server. Is it running?"
        is ChatException.Http ->
            if (error.status >= 500u) {
                "The chat server is having problems. Try again."
            } else {
                "The chat server refused the request."
            }
        is ChatException.BadResponse -> "The chat server sent something this app can't read."
        is ChatException.Cancelled -> "Cancelled."
        is ChatException.NotFound -> "That conversation no longer exists."
        is ChatException.UnsupportedOnHost ->
            "This device can't do that yet. Android supports http:// chat servers only."
        is ChatException.InvalidInput -> "That can't be sent."
        is ChatException.Storage -> "Saved chats can't be read by this version of the app."
        is ChatException.ShutDown -> "Chat has been closed. Reopen the app to continue."
        is ChatException.Internal -> "Something went wrong in the chat engine."
        is CancellationException -> "Cancelled."
        else -> "Something went wrong: ${error.message ?: error::class.simpleName}"
    }
