package org.forgemobile.chat

import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import uniffi.forge_ffi.ChatAppConfig
import uniffi.forge_ffi.ChatConversation
import uniffi.forge_ffi.ChatEvent
import uniffi.forge_ffi.ChatException
import uniffi.forge_ffi.ChatMessage
import uniffi.forge_ffi.ChatMessageStatus
import uniffi.forge_ffi.ForgeChatApp

/**
 * Drives [ChatSession] against the real Rust core through the generated Kotlin bindings, and
 * the demo interaction against the real mock server. Needs the host builds:
 * `cargo build -p forge-ffi -p mock-chat-server` from the workspace root.
 */
class ChatSessionTest {
    private lateinit var dir: File
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val sessions = mutableListOf<ChatSession>()
    private val servers = mutableListOf<MockServer>()

    @Before
    fun setUp() {
        dir = temporaryDirectory()
    }

    @After
    fun tearDown() {
        sessions.forEach { it.shutdown() }
        servers.forEach { it.stop() }
        scope.cancel()
        dir.deleteRecursively()
    }

    private fun app(url: String, timeoutMs: UInt = 2_000u) =
        ForgeChatApp(
            dir.absolutePath,
            ChatAppConfig(baseUrl = url, user = null, requestTimeoutMs = timeoutMs),
        )

    private fun session(url: String): ChatSession =
        ChatSession(app(url), scope).also { sessions += it }

    private fun server(port: Int = 0): MockServer = MockServer.start(port).also { servers += it }

    private fun message(id: String, sentAt: Long, status: ChatMessageStatus = ChatMessageStatus.SENT) =
        ChatMessage(
            id = id,
            conversationId = "general",
            author = "someone",
            body = "body $id",
            sentAt = sentAt,
            status = status,
        )

    @Test
    fun badServerUrlIsInvalidInput() {
        try {
            app("ftp://example.com")
            fail("expected InvalidInput")
        } catch (e: ChatException.InvalidInput) {
            assertEquals("That can't be sent.", userMessage(e))
        }
    }

    @Test
    fun serverDownShowsAListErrorAndKeepsGoing() = runBlocking {
        val chat = session("http://127.0.0.1:${MockServer.unusedPort()}")
        chat.start()
        val state = chat.state.value
        assertEquals("Can't reach the chat server. Is it running?", state.listError)
        assertTrue(state.conversations.isEmpty())
        assertTrue(!state.isRefreshing)
    }

    @Test
    fun failedSendShowsAFailedBubbleAndAnError() = runBlocking {
        val chat = session("http://127.0.0.1:${MockServer.unusedPort()}")
        chat.start()
        chat.send("hello", "general")
        waitUntil("the failed bubble shows") {
            chat.state.value.messages("general").any { it.status == ChatMessageStatus.FAILED }
        }
        assertNotNull(chat.state.value.sendError["general"])
    }

    @Test
    fun blankDraftsAreNotSent() = runBlocking {
        val chat = session("http://127.0.0.1:${MockServer.unusedPort()}")
        chat.send("   \n", "general")
        assertTrue(chat.app.cachedMessages("general").isEmpty())
        assertNull(chat.state.value.sendError["general"])
    }

    @Test
    fun eventsUpsertAndOrderMessages() {
        val chat = session("http://127.0.0.1:1")
        chat.apply(ChatEvent.MessageReceived(message("b", 20)))
        chat.apply(ChatEvent.MessageReceived(message("a", 10)))
        chat.apply(ChatEvent.MessageReceived(message("b", 20).copy(body = "edited")))
        val list = chat.state.value.messages("general")
        assertEquals(listOf("a", "b"), list.map { it.id })
        assertEquals("edited", list[1].body)
    }

    @Test
    fun statusChangesApplyOnlyToTheirRoom() {
        val chat = session("http://127.0.0.1:1")
        chat.apply(ChatEvent.MessageReceived(message("c", 30, ChatMessageStatus.SENDING)))
        chat.apply(ChatEvent.MessageStatusChanged("general", "c", ChatMessageStatus.FAILED))
        assertEquals(ChatMessageStatus.FAILED, chat.state.value.messages("general").last().status)
        // Another room's event leaves this one alone.
        chat.apply(ChatEvent.MessageStatusChanged("random", "c", ChatMessageStatus.SENT))
        assertEquals(ChatMessageStatus.FAILED, chat.state.value.messages("general").last().status)
    }

    @Test
    fun connectionStateFollowsStreamEvents() {
        val chat = session("http://127.0.0.1:1")
        chat.apply(ChatEvent.StreamConnected("general"))
        assertEquals(ConnectionState.Connected, chat.state.value.connection["general"])
        chat.apply(ChatEvent.StreamDisconnected("general", "eof", willRetry = true))
        assertEquals(ConnectionState.Reconnecting("eof"), chat.state.value.connection["general"])
        chat.apply(ChatEvent.StreamDisconnected("general", "left", willRetry = false))
        assertEquals(ConnectionState.Closed, chat.state.value.connection["general"])
    }

    @Test
    fun conversationUpdatesReplaceById() {
        val chat = session("http://127.0.0.1:1")
        chat.apply(ChatEvent.ConversationUpdated(ChatConversation("general", "General", null, 0u)))
        chat.apply(ChatEvent.ConversationUpdated(ChatConversation("general", "General", 5, 2u)))
        val list = chat.state.value.conversations
        assertEquals(1, list.size)
        assertEquals(2u, list[0].unreadCount)
    }

    @Test
    fun httpsIsAllowedOnTheJvmHost() {
        // The UnsupportedOnHost guard applies to Android builds of the Rust library only; the
        // JVM tests load the host build, so this proves the guard does not leak onto desktops.
        app("https://chat.example").shutdown()
    }

    /** The README's demo, end to end: send, leave, kill the server, fail, restart, retry. */
    @Test
    fun demoInteractionAgainstTheMockServer() = runBlocking {
        val first = server()
        val chat = session(first.url)
        chat.start()
        assertEquals(listOf("general", "random", "forge"), chat.state.value.conversations.map { it.id })

        // 1. Open General and send: Sending, then Sent.
        chat.open("general")
        waitUntil("connected") {
            chat.state.value.connection["general"] == ConnectionState.Connected
        }
        chat.send("hello from kotlin", "general")
        waitUntil("the message is sent") {
            chat.state.value.messages("general").any {
                it.body == "hello from kotlin" && it.status == ChatMessageStatus.SENT
            }
        }

        // 6. Someone else posts while the room is open: it arrives over the stream.
        first.post("general", "curl", "hello from curl")
        waitUntil("the curl message arrives") {
            chat.state.value.messages("general").any { it.author == "curl" }
        }

        // 3. Kill the server: the room reconnects on its own.
        first.stop()
        waitUntil("reconnecting") {
            chat.state.value.connection["general"] is ConnectionState.Reconnecting
        }

        // 4. Sending now fails, as a red bubble.
        chat.send("while down", "general")
        val failed =
            run {
                waitUntil("the send fails") {
                    chat.state.value.messages("general").any { it.status == ChatMessageStatus.FAILED }
                }
                chat.state.value.messages("general").first { it.status == ChatMessageStatus.FAILED }
            }

        // 5. Restart on the same port and tap the bubble: Sent, and the room reconnects.
        server(first.port)
        chat.retry(failed)
        waitUntil("the retry is sent") {
            chat.state.value.messages("general").any {
                it.id == failed.id && it.status == ChatMessageStatus.SENT
            }
        }
        waitUntil("reconnected") {
            chat.state.value.connection["general"] == ConnectionState.Connected
        }

        // 2. Leaving the room stops the Rust thread behind it.
        chat.close("general")
        waitUntil("the room's stream thread ends") { chat.app.liveStreams() == 0u }
        waitUntil("the room shows as closed") {
            chat.state.value.connection["general"] == ConnectionState.Closed
        }
    }
}
