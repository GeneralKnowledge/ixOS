package org.forgemobile.relay

import java.io.File
import java.nio.file.Files
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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.relay_core.CodeSend
import uniffi.relay_core.Direction
import uniffi.relay_core.Item
import uniffi.relay_core.NearbyDevice
import uniffi.relay_core.PairingPolicy
import uniffi.relay_core.RelayException
import uniffi.relay_core.TransferStatus

/**
 * The Android host and model against Relay's real Rust core (the host build of rust/,
 * `cargo build --manifest-path rust/Cargo.toml`), the same core the iPhone app runs. Two
 * devices in one process reach each other over loopback, as Relay's own Rust tests do.
 */
class RelayModelTest {
    private val root = Files.createTempDirectory("relay-android").toFile()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val hosts = mutableListOf<ForgeHost<RelayHostCore>>()

    @After
    fun close() {
        hosts.forEach { it.close() }
        scope.cancel()
        root.deleteRecursively()
    }

    /** A device on throwaway data with no servers, opened through the host as the app does. */
    private fun device(name: String): RelayModel = runBlocking {
        val env = ForgeEnvironment.temporary(root)
        val host = ForgeHost(
            scope = scope,
            environment = { env },
            open = { RelayOpener { name }.open(it, offline = true) },
        )
        hosts += host
        host.start()
        assertEquals(ForgeHost.Status.Ready, host.status.value)
        RelayModel(host.core!!.core)
    }

    private fun file(name: String, text: String): String =
        File(root, name).apply { writeText(text) }.absolutePath

    /** Sends [path] from [from] to [to] with a word code, as the two screens do. */
    private suspend fun codeTransfer(from: RelayModel, to: RelayModel, path: String) {
        from.send(listOf(path))
        val waiting = withTimeout(20_000) {
            var now = from.codeSend.value
            while (now !is CodeSend.Waiting) {
                delay(50)
                from.refreshCodeSend()
                now = from.codeSend.value
            }
            now
        }
        to.receive(waiting.code) { number ->
            assertEquals(waiting.number, number)
            CodeSender.Found(listOf("127.0.0.1:${waiting.port}"), waiting.endpointId)
        }
        withTimeout(20_000) {
            while (from.codeSend.value !is CodeSend.Sent) {
                delay(50)
                from.refreshCodeSend()
            }
        }
    }

    @Test
    fun aNewDeviceIsNamedAndEmpty() {
        val phone = device("Pixel 9")
        val state = phone.state.value
        assertEquals("Pixel 9", state.identity.name)
        assertTrue(state.isEmpty)
        assertEquals(PairingPolicy.ASK_TO_PAIR, state.policy)
        assertEquals(ServerChoice.NONE, ServerChoice.of(state.servers))
    }

    @Test
    fun renamingIsCheckedByRust() = runBlocking {
        val phone = device("Pixel")
        phone.rename("Reuben's Pixel")
        assertEquals("Reuben's Pixel", phone.state.value.identity.name)
        val refused = runCatching { phone.rename("  ") }.exceptionOrNull()
        assertTrue("$refused", refused is RelayException.EmptyName)
    }

    @Test
    fun aBadCodeIsSaidBeforeLooking() = runBlocking {
        val phone = device("Pixel")
        var looked = false
        phone.receive("not a code") { looked = true; CodeSender.None }
        assertTrue(phone.codeReceive.value is CodeReceive.Failed)
        assertTrue(!looked)
    }

    @Test
    fun noSenderOnTheNetworkIsSaid() = runBlocking {
        val phone = device("Pixel")
        phone.receive("7-acid-tidy-rug") { CodeSender.None }
        val failed = phone.codeReceive.value as CodeReceive.Failed
        assertTrue(failed.message, failed.message.startsWith("No sender with code number 7"))
    }

    @Test
    fun aCodeTransferThenTrustThenSendingWithoutACode() = runBlocking {
        val phone = device("Pixel")
        val laptop = device("Laptop")

        codeTransfer(laptop, phone, file("hello.txt", "hello from the laptop"))

        // The phone received it, and offers to trust the laptop.
        val received = phone.codeReceive.value as CodeReceive.Received
        assertEquals("hello.txt", received.receipt.name)
        assertEquals("hello from the laptop", File(received.receipt.path).readText())
        val offer = phone.state.value.trustOffer
        assertNotNull(offer)
        offer!!
        assertEquals("Laptop", offer.name)
        phone.trust(offer)
        assertNull(phone.state.value.trustOffer)
        assertEquals(listOf("Laptop"), phone.state.value.trustedDevices.map { it.name })

        // The laptop trusts the phone back.
        laptop.refreshTrustOffer()
        val back = laptop.state.value.trustOffer!!
        assertEquals("Pixel", back.name)
        laptop.trust(back)

        // The laptop finds the phone on the network, and sends to it without a code.
        val service = phone.deviceService()!!
        laptop.devicesNearby(listOf(NearbyDevice(service.endpointId, listOf("127.0.0.1:${service.port}"))))
        val phoneThere = laptop.state.value.trustedDevices.single()
        assertTrue(laptop.send(listOf(file("note.txt", "no code needed")), phoneThere))

        withTimeout(20_000) {
            while (phone.state.value.transfers.none { it.direction == Direction.RECEIVED && (it.item as? Item.File)?.name == "note.txt" && it.status is TransferStatus.Done }) {
                delay(100)
                phone.reload()
            }
        }
    }

    @Test
    fun revokingAsksFirstAndIsFinal() = runBlocking {
        val phone = device("Pixel")
        val laptop = device("Laptop")
        codeTransfer(laptop, phone, file("a.txt", "a"))
        phone.trust(phone.state.value.trustOffer!!)
        val trusted = phone.state.value.trustedDevices.single()

        phone.askToRevoke(trusted)
        val ask = phone.alert.value!!
        assertEquals("Revoke “Laptop”?", ask.title)
        phone.handle(ask.action("Revoke")!!)

        assertTrue(phone.state.value.trustedDevices.isEmpty())
        assertEquals(listOf("Laptop"), phone.state.value.revokedDevices.map { it.name })
    }
}
