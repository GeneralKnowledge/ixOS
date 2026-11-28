// What the Chats screens say, apart from the screens so JVM tests check the words: the same
// words as the iPhone app (ConversationListView.swift, ChatView.swift).
package org.forgemobile.relay

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import uniffi.relay_core.ChatConversation
import uniffi.relay_core.ChatDelivery
import uniffi.relay_core.ChatMessage
import uniffi.relay_core.ChatMessageStatus
import uniffi.relay_core.deviceConversationId

object ChatText {
    /** Quick reactions offered on a message's menu, as on the iPhone. */
    val quickReactions = listOf("👍", "❤️", "😂", "😮", "😢", "🙏")

    fun waiting(count: Int): String =
        if (count == 1) {
            "1 message is waiting to be sent. It goes out when the other side can be reached."
        } else {
            "$count messages are waiting to be sent. They go out when the other side can be reached."
        }

    /** The row's second line: who wrote the last message, and what. */
    fun preview(conversation: ChatConversation): String {
        val last = conversation.lastMessage ?: return "No messages yet"
        val who = if (last.isMine) "You" else last.author
        if (last.deleted) return "$who: Message deleted"
        return when (last.status) {
            ChatMessageStatus.FAILED -> "Not sent: ${ChatModel.summary(last)}"
            ChatMessageStatus.SENDING, ChatMessageStatus.SENT -> "$who: ${ChatModel.summary(last)}"
        }
    }

    /** What TalkBack reads for a row. */
    fun rowDescription(conversation: ChatConversation): String {
        val parts = mutableListOf(conversation.title)
        if (conversation.unreadCount > 0u) {
            parts += "${conversation.unreadCount} unread"
        } else if (conversation.markedUnread) {
            parts += "marked unread"
        }
        if (conversation.muted) parts += "muted"
        parts += preview(conversation)
        return parts.joinToString(", ")
    }

    /** The list's time column: the time today, "Yesterday", the weekday within a week, else the date. */
    fun listTime(ms: Long, now: Instant = Instant.now(), zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String {
        val at = Instant.ofEpochMilli(ms).atZone(zone)
        val today = now.atZone(zone).toLocalDate()
        val day = at.toLocalDate()
        return when {
            day == today -> at.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale))
            day == today.minusDays(1) -> "Yesterday"
            ChronoUnit.DAYS.between(day, today) in 0..6 -> day.dayOfWeek.getDisplayName(TextStyle.FULL, locale)
            else -> day.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.SHORT).withLocale(locale))
        }
    }

    /** A separator in the transcript: "Today 14:05", "Yesterday 09:30", "Monday 18:00", or the date. */
    fun separator(ms: Long, now: Instant = Instant.now(), zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String {
        val at = Instant.ofEpochMilli(ms).atZone(zone)
        val time = at.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale))
        val today: LocalDate = now.atZone(zone).toLocalDate()
        val day = at.toLocalDate()
        return when {
            day == today -> "Today $time"
            day == today.minusDays(1) -> "Yesterday $time"
            ChronoUnit.DAYS.between(day, today) in 0..6 -> "${day.dayOfWeek.getDisplayName(TextStyle.FULL, locale)} $time"
            else -> "${day.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale))} $time"
        }
    }

    fun time(ms: Long, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
        Instant.ofEpochMilli(ms).atZone(zone).format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale))

    /** Under our newest message, or one still on its way. */
    fun delivery(delivery: ChatDelivery): String = when (delivery) {
        ChatDelivery.SENDING -> "Sending…"
        ChatDelivery.SENT -> "Delivered"
        ChatDelivery.READ -> "Read"
        ChatDelivery.IN_MAILBOX -> "In the Mailbox"
        ChatDelivery.FAILED -> "Not Delivered. Tap to Try Again."
    }

    const val DELETED = "This message was deleted"

    /**
     * Whether [conversationId] is a trusted device's conversation. Relay's Rust core carries
     * those over its outbox, which can't yet edit, react to or delete what was delivered
     * (chat-core answers UnsupportedOnHost), so the message menu leaves those out.
     */
    fun isDevice(conversationId: String): Boolean = conversationId.startsWith(DEVICE_PREFIX)

    private val DEVICE_PREFIX = deviceConversationId("x").removeSuffix("x")

    /** Whether the menu offers Edit and reactions on [message]. */
    fun canChange(message: ChatMessage): Boolean = !message.deleted && !isDevice(message.conversationId)

    /** Whether the menu offers Delete on [message]: ours, and, in a device's, never delivered. */
    fun canUnsend(message: ChatMessage): Boolean =
        message.isMine && !message.deleted &&
            (!isDevice(message.conversationId) || message.status == ChatMessageStatus.FAILED)

    /** The composer's count once the text is near the limit, else null. */
    fun remaining(text: String): String? {
        val left = MAX_MESSAGE_CHARS - text.codePointCount(0, text.length)
        return if (left < 200) "$left" else null
    }

    /** Where the transcript breaks: a new group after 5 minutes, a time separator after an hour. */
    const val GROUP_GAP_MS = 5 * 60 * 1000L
    const val SEPARATOR_GAP_MS = 60 * 60 * 1000L

    /** How one message sits in the transcript. */
    data class Place(
        /** A time separator above it. */
        val separator: Boolean,
        /** Its author's name above it (someone else's, starting a group). */
        val showsAuthor: Boolean,
        /** Its delivery under it (ours, still on its way or failed, or the newest sent). */
        val showsDelivery: Boolean,
    )

    /** The places of [messages] (oldest first), as the iPhone transcript groups them. */
    fun places(messages: List<ChatMessage>): List<Place> {
        val newestSentMine = messages.indexOfLast { it.isMine && it.status == ChatMessageStatus.SENT }
        return messages.mapIndexed { i, message ->
            val before = messages.getOrNull(i - 1)
            val separator = before == null || message.sentAt - before.sentAt >= SEPARATOR_GAP_MS
            val newGroup = separator || before == null || before.authorId != message.authorId ||
                message.sentAt - before.sentAt >= GROUP_GAP_MS
            Place(
                separator = separator,
                showsAuthor = newGroup && !message.isMine,
                showsDelivery = message.isMine && (message.status != ChatMessageStatus.SENT || i == newestSentMine),
            )
        }
    }
}

/**
 * Where a message notification leads: `forgechat://conversation/<id>?message=<id>`, as Rust
 * writes it (`chatMessageLink`). The iPhone's ChatLinks.swift reads the same links.
 */
data class ChatLink(val conversationId: String, val messageId: String?) {
    companion object {
        const val SCHEME = "forgechat"

        /** The conversation [link] points at, or null if it isn't a Chats link. */
        fun parse(link: String): ChatLink? {
            val uri = try {
                java.net.URI(link)
            } catch (e: java.net.URISyntaxException) {
                return null
            }
            if (uri.scheme != SCHEME || uri.host != "conversation") return null
            val parts = (uri.rawPath ?: "").split('/').filter { it.isNotEmpty() }
            if (parts.size != 1) return null
            val message = (uri.rawQuery ?: "").split('&')
                .map { it.split('=', limit = 2) }
                .firstOrNull { it.size == 2 && it[0] == "message" }
                ?.get(1)
            return ChatLink(decode(parts[0]), message?.let(::decode))
        }

        private fun decode(text: String): String = java.net.URLDecoder.decode(text.replace("+", "%2B"), "UTF-8")
    }
}
