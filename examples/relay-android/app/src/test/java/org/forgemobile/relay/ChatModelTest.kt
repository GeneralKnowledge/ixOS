package org.forgemobile.relay

import java.io.File
import java.nio.file.Files
import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.forgemobile.relay.host.ForgeEnvironment
import org.forgemobile.relay.host.ForgeHost
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.relay_core.ChatMessage
import uniffi.relay_core.ChatMessageStatus
import uniffi.relay_core.ChatOutgoingFile
import uniffi.relay_core.CodeSend
import uniffi.relay_core.NearbyDevice
import uniffi.relay_core.deviceConversationId

/**
 * The Chats tab's model against Relay's real Rust core (the host build), as the iPhone's
 * ChatModel runs it: each trusted device is a conversation, and messages go through Relay's
 * one outbox. Two devices in one process reach each other over loopback.
 */
class ChatModelTest {
    private val root = Files.createTempDirectory("relay-chat").toFile()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val hosts = mutableListOf<ForgeHost<RelayHostCore>>()
    private val chats = mutableListOf<ChatModel>()

    @After
    fun close() {
        chats.forEach { it.close() }
        hosts.forEach { it.close() }
        scope.cancel()
        root.deleteRecursively()
    }

    private class Device(val relay: RelayModel, val chat: ChatModel)

    private fun device(name: String): Device = runBlocking {
        val env = ForgeEnvironment.temporary(root)
        val host = ForgeHost(
            scope = scope,
            environment = { env },
            open = { RelayOpener { name }.open(it, offline = true) },
        )
        hosts += host
        host.start()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
        val core = host.core!!
        assertNotNull("Chats didn't open: ${core.chatProblem}", core.chat)
        val chat = ChatModel(core.chat!!, core.core, scope)
        chats += chat
        Device(RelayModel(core.core), chat)
    }

    private fun file(name: String, text: String): String =
        File(root, name).apply { writeText(text) }.absolutePath

    /** A phone and a laptop that made one code transfer, trust each other and see each other. */
    private suspend fun trustedPair(): Pair<Device, Device> {
        val phone = device("Pixel")
        val laptop = device("Laptop")
        laptop.relay.send(listOf(file("hello.txt", "hello")))
        val waiting = withTimeout(20_000) {
            var now = laptop.relay.codeSend.value
            while (now !is CodeSend.Waiting) {
                delay(50)
                laptop.relay.refreshCodeSend()
                now = laptop.relay.codeSend.value
            }
            now
        }
        phone.relay.receive(waiting.code) { CodeSender.Found(listOf("127.0.0.1:${waiting.port}"), waiting.endpointId) }
        phone.relay.trust(phone.relay.state.value.trustOffer!!)
        withTimeout(20_000) {
            while (laptop.relay.state.value.trustOffer == null) {
                delay(50)
                laptop.relay.refreshTrustOffer()
            }
        }
        laptop.relay.trust(laptop.relay.state.value.trustOffer!!)
        for ((from, to) in listOf(phone to laptop, laptop to phone)) {
            val service = to.relay.deviceService()!!
            from.relay.devicesNearby(listOf(NearbyDevice(service.endpointId, listOf("127.0.0.1:${service.port}"))))
        }
        return phone to laptop
    }

    private suspend fun waitFor(what: String, check: suspend () -> Boolean) {
        try {
            withTimeout(30_000) {
                while (!check()) delay(100)
            }
        } catch (e: Exception) {
            throw AssertionError("timed out waiting for $what", e)
        }
    }

    @Test
    fun withNoTrustedDevicesThereAreNoConversations() = runBlocking {
        val phone = device("Pixel")
        phone.chat.refresh()
        val state = phone.chat.state.value
        assertTrue(state.hasListed)
        assertTrue(state.conversations.isEmpty())
        assertEquals(null, state.listError)
    }

    @Test
    fun aTrustedDeviceIsAConversationThatCarriesMessagesBothWays() = runBlocking {
        val (phone, laptop) = trustedPair()
        val laptopId = phone.relay.state.value.trustedDevices.single().endpointId
        val phoneId = laptop.relay.state.value.trustedDevices.single().endpointId
        val toLaptop = deviceConversationId(laptopId)
        val toPhone = deviceConversationId(phoneId)

        phone.chat.refresh()
        assertEquals(listOf("Laptop"), phone.chat.state.value.conversations.map { it.title })

        phone.chat.enter(toLaptop)
        assertTrue(phone.chat.send("hello laptop", toLaptop))
        // The bubble shows at once, from Rust's event.
        waitFor("the phone's own bubble") { phone.chat.state.value.messages(toLaptop).any { it.body == "hello laptop" } }

        // It arrives on the laptop, in the phone's conversation.
        waitFor("the laptop to receive it") {
            laptop.chat.refresh()
            laptop.chat.state.value.conversation(toPhone)?.lastMessage?.body == "hello laptop"
        }
        laptop.chat.enter(toPhone)
        val received = laptop.chat.state.value.messages(toPhone).single { it.body == "hello laptop" }
        assertFalse(received.isMine)

        // A reply, quoting it, comes back to the phone.
        assertTrue(laptop.chat.send("hello phone", toPhone, replyTo = received.id))
        waitFor("the phone to receive the reply") {
            phone.chat.state.value.messages(toLaptop).any { it.body == "hello phone" }
        }
        val reply = phone.chat.state.value.messages(toLaptop).single { it.body == "hello phone" }
        assertNotNull(reply.replyTo)

        // The phone's own message ends up sent.
        waitFor("the phone's message to be sent") {
            phone.chat.state.value.messages(toLaptop).single { it.body == "hello laptop" }.status == ChatMessageStatus.SENT
        }
        assertEquals(0, phone.chat.state.value.waitingCount)
    }

    @Test
    fun aFileGoesAsAnAttachment() = runBlocking {
        val (phone, laptop) = trustedPair()
        val toLaptop = deviceConversationId(phone.relay.state.value.trustedDevices.single().endpointId)
        val toPhone = deviceConversationId(laptop.relay.state.value.trustedDevices.single().endpointId)
        phone.chat.enter(toLaptop)
        val path = file("plan.txt", "the plan")
        assertTrue(phone.chat.send(listOf(ChatOutgoingFile(path, "plan.txt", "text/plain")), "see this", toLaptop))

        // Each file goes through Relay's outbox as a message of its own; the caption follows.
        laptop.chat.enter(toPhone)
        waitFor("the attachment and caption on the laptop") {
            laptop.chat.enter(toPhone)
            val list = laptop.chat.state.value.messages(toPhone)
            list.any { it.attachments.isNotEmpty() } && list.any { it.body == "see this" }
        }
        val message = laptop.chat.state.value.messages(toPhone).single { it.attachments.isNotEmpty() }
        assertEquals("Photo or file messages carry no text", "", message.body)
        val attachment = message.attachments.single()
        assertEquals("plan.txt", attachment.name)
        var local: String? = null
        waitFor("the attachment's file") {
            local = laptop.chat.localFile(attachment)
            local != null
        }
        assertEquals("the plan", File(local!!).readText())
    }

    @Test
    fun tooLongIsRefusedBeforeKeepingSoTheTextComesBack() = runBlocking {
        val (phone, _) = trustedPair()
        val toLaptop = deviceConversationId(phone.relay.state.value.trustedDevices.single().endpointId)
        phone.chat.enter(toLaptop)
        val long = "a".repeat(MAX_MESSAGE_CHARS + 1)
        assertFalse(phone.chat.send(long, toLaptop))
        assertNotNull(phone.chat.state.value.sendError[toLaptop])
        assertTrue(phone.chat.state.value.messages(toLaptop).none { it.body == long })

        // A caption too long refuses its files too, before any goes: the screen keeps them.
        val path = file("plan.txt", "the plan")
        assertFalse(phone.chat.send(listOf(ChatOutgoingFile(path, "plan.txt", "text/plain")), long, toLaptop))
        assertTrue(phone.chat.state.value.messages(toLaptop).none { it.attachments.isNotEmpty() })
    }

    @Test
    fun deletingAsksFirstAndADeviceSaysItCantUnsend() = runBlocking {
        val (phone, _) = trustedPair()
        val toLaptop = deviceConversationId(phone.relay.state.value.trustedDevices.single().endpointId)
        phone.chat.enter(toLaptop)
        phone.chat.send("oops", toLaptop)
        waitFor("the bubble") { phone.chat.state.value.messages(toLaptop).any { it.body == "oops" } }
        val message = phone.chat.state.value.messages(toLaptop).single { it.body == "oops" }
        phone.chat.askToDelete(message)
        val ask = phone.chat.alert.value!!
        assertTrue(ask.title, ask.title.startsWith("Delete this"))
        // A device conversation can't take back what it delivered (Rust has no unsend for
        // devices yet), so the screen never offers it; asked anyway, Rust refuses. (Its words
        // are "Sign up first.", from the chat server's route: see relay-android.md.)
        waitFor("the message sent") {
            phone.chat.state.value.messages(toLaptop).single { it.body == "oops" }.status == ChatMessageStatus.SENT
        }
        assertFalse(ChatText.canUnsend(phone.chat.state.value.messages(toLaptop).single { it.body == "oops" }))
        phone.chat.handle(ask.action("Delete")!!)
        assertNotNull(phone.chat.alert.value)
        assertTrue(phone.chat.state.value.messages(toLaptop).any { it.body == "oops" })
    }

    @Test
    fun draftsAreKept() = runBlocking {
        val (phone, _) = trustedPair()
        val toLaptop = deviceConversationId(phone.relay.state.value.trustedDevices.single().endpointId)
        phone.chat.saveDraft("half a thought", toLaptop)
        assertEquals("half a thought", phone.chat.draft(toLaptop))
        phone.chat.saveDraft("", toLaptop)
        assertEquals("", phone.chat.draft(toLaptop))
    }

    // The words, without a core.

    private fun message(
        id: String,
        at: Long,
        mine: Boolean = false,
        author: String = "Laptop",
        status: ChatMessageStatus = ChatMessageStatus.SENT,
        body: String = "hi",
    ) = ChatMessage(
        id = id, conversationId = "c", author = author, authorId = if (mine) "me" else author, isMine = mine,
        body = body, sentAt = at, status = status, replyTo = null, edited = false, deleted = false,
        reactions = emptyList(), attachments = emptyList(), inMailbox = false,
    )

    @Test
    fun theTranscriptGroupsByAuthorAndTime() {
        val minute = 60_000L
        val places = ChatText.places(
            listOf(
                message("1", 0),
                message("2", minute),
                message("3", 2 * minute, mine = true),
                message("4", 3 * minute, mine = true),
                message("5", 70 * minute),
                message("6", 71 * minute, mine = true, status = ChatMessageStatus.SENDING),
            ),
        )
        assertEquals(listOf(true, false, false, false, true, false), places.map { it.separator })
        assertEquals(listOf(true, false, false, false, true, false), places.map { it.showsAuthor })
        // Only the newest sent of ours, and anything of ours still on its way.
        assertEquals(listOf(false, false, false, true, false, true), places.map { it.showsDelivery })
    }

    @Test
    fun theListSaysWhenInTheWeek() {
        val zone = ZoneId.of("Europe/London")
        val now = Instant.parse("2026-11-22T15:00:00Z")
        val day = 24 * 3600_000L
        assertEquals("Yesterday", ChatText.listTime(now.toEpochMilli() - day, now, zone, Locale.UK))
        assertEquals("Tuesday", ChatText.listTime(now.toEpochMilli() - 5 * day, now, zone, Locale.UK))
        assertEquals("15:00", ChatText.listTime(now.toEpochMilli(), now, zone, Locale.UK))
    }

    @Test
    fun aPhotoWithoutTextIsSummarised() {
        val photo = message("1", 0, body = "").copy(
            attachments = listOf(uniffi.relay_core.ChatAttachment("a", "IMG_1.jpg", "image/jpeg", 10u, true)),
        )
        assertEquals("Photo", ChatModel.summary(photo))
        assertEquals("You: hi", ChatText.preview(
            uniffi.relay_core.ChatConversation("c", "Laptop", 0, 0u, false, message("1", 0, mine = true), null, false, false),
        ))
    }
}
