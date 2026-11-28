package org.forgemobile.relay

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.relay_core.Capability
import uniffi.relay_core.CodeSend
import uniffi.relay_core.DeviceService
import uniffi.relay_core.Device
import uniffi.relay_core.DevicesSynced
import uniffi.relay_core.Direction
import uniffi.relay_core.Item
import uniffi.relay_core.NearbySender
import uniffi.relay_core.Presence
import uniffi.relay_core.RelayException
import uniffi.relay_core.RelayServers
import uniffi.relay_core.Transfer
import uniffi.relay_core.TransferStatus
import uniffi.relay_core.ownRelayServers

/** Relay's words and adverts on Android: the same as the iPhone app's. */
class RelayTextTest {
    private fun device(
        presence: Presence = Presence.OFFLINE,
        lastSeenMs: Long? = null,
        nearby: Boolean = false,
        problem: String? = null,
        route: String? = null,
        person: String? = null,
    ) = Device(
        endpointId = "abc", name = "Pixel", trusted = true, sinceMs = 0, receivesFrom = true, mayAsk = false,
        introduces = false, seesDevices = false, introducedBy = null, revokedBy = null, person = person,
        nearby = nearby, presence = presence, lastSeenMs = lastSeenMs, platform = null, offers = emptyList(),
        route = route, problem = problem, grants = listOf(Capability.SITES),
    )

    @Test
    fun sizesReadAsOnTheIPhone() {
        assertEquals("512 bytes", RelayFormat.size(512u))
        assertEquals("1.5 KB", RelayFormat.size(1_500u))
        assertEquals("2.1 MB", RelayFormat.size(2_100_000u))
        assertEquals("250 MB", RelayFormat.size(250_000_000u))
        assertEquals("1.25 GB", RelayFormat.size(1_250_000_000u))
    }

    @Test
    fun aTransferSaysWhereItIs() {
        fun transfer(status: TransferStatus, direction: Direction = Direction.SENT) = Transfer(
            id = 1u, peerId = "abc", peerName = "Mac", direction = direction,
            item = Item.File("a.txt", 200u, null), status = status, atMs = 0,
        )
        assertEquals("Waiting for Mac", RelayFormat.status(transfer(TransferStatus.Waiting)))
        assertEquals("In the Mailbox", RelayFormat.status(transfer(TransferStatus.InMailbox)))
        assertEquals("Sending 50%", RelayFormat.status(transfer(TransferStatus.InProgress(100u))))
        assertEquals("Receiving 25%", RelayFormat.status(transfer(TransferStatus.InProgress(50u), Direction.RECEIVED)))
        assertEquals("Sent", RelayFormat.status(transfer(TransferStatus.Done)))
        assertEquals("Failed: gone", RelayFormat.status(transfer(TransferStatus.Failed("gone"))))
        assertEquals("To Mac", RelayFormat.peer(transfer(TransferStatus.Done)))
    }

    @Test
    fun presenceAges() {
        val now = 10_000_000L
        assertEquals("Nearby", RelayFormat.whereNow(device(Presence.NEARBY), now))
        assertEquals("Offline, not reached yet", RelayFormat.whereNow(device(), now))
        assertEquals("Reachable, seen just now", RelayFormat.whereNow(device(Presence.REACHABLE, now - 5_000), now))
        assertEquals("Offline, seen 2 min ago", RelayFormat.whereNow(device(lastSeenMs = now - 125_000), now))
        assertEquals("Offline, seen 2 h ago", RelayFormat.whereNow(device(lastSeenMs = now - 7_300_000), now))
    }

    @Test
    fun reachPrefersNearbyThenTheProblem() {
        assertEquals("Pixel is nearby. No code needed.", RelayFormat.reach(device(nearby = true, problem = "x")))
        assertEquals("No route.", RelayFormat.reach(device(problem = "No route.")))
        assertEquals(
            "Last connected: Wi-Fi. Relay tries it there again when something is waiting.",
            RelayFormat.reach(device(route = "Wi-Fi")),
        )
        assertEquals("Sam (Pixel)", RelayFormat.rowName(device(person = "Sam")))
    }

    @Test
    fun syncedSaysWhatChanged() {
        assertEquals("Nothing new from Mac.", RelayFormat.synced(DevicesSynced(0u, 0u), "Mac"))
        assertEquals(
            "From Mac: 2 devices to trust, 1 device revoked.",
            RelayFormat.synced(DevicesSynced(2u, 1u), "Mac"),
        )
    }

    @Test
    fun addressesAreWhatRustTakes() {
        assertEquals("192.168.1.5:7000", RelayFormat.address("192.168.1.5", 7000))
        assertEquals("[fe80::1]:7000", RelayFormat.address("fe80::1%wlan0", 7000))
    }

    @Test
    fun errorsReadAsOnTheIPhone() {
        assertEquals("Check the Code", RelayErrors.title(RelayException.WordCode("bad")))
        assertEquals("The code has expired.", RelayErrors.message(RelayException.WordCode("the code has expired")))
        assertEquals("Not Sent", RelayErrors.failure(RelayException.Unreachable("x"))!!.title)
        assertEquals(true, RelayErrors.failure(RelayException.Unreachable("x"))!!.isRetryable)
        assertNull(RelayErrors.failure(IllegalStateException()))
    }

    @Test
    fun serverChoices() {
        assertEquals(ServerChoice.NONE, ServerChoice.of(RelayServers(null, null)))
        assertEquals(ServerChoice.RELAYS, ServerChoice.of(ownRelayServers()))
        assertEquals(ServerChoice.OWN, ServerChoice.of(RelayServers("https://relay.example", null)))
        assertEquals(RelayServers("https://r", null), ServerChoice.own(" https://r ", "  "))
    }

    @Test
    fun advertsMatchTheIPhoneAndTheCommand() {
        val code = CodeAdvert(7u, 4242u, "0123456789abcdef")
        assertEquals("relay-01234567-7", RelayAdverts.codeName(code))
        assertEquals(mapOf("v" to "1", "n" to "7", "id" to "0123456789abcdef"), RelayAdverts.codeAttributes(code))
        val service = DeviceService("0123456789abcdef", 4243u, emptyList())
        assertEquals("relay-0123456789ab", RelayAdverts.deviceName(service))
        assertEquals(mapOf("v" to "1", "d" to "1", "id" to "0123456789abcdef"), RelayAdverts.deviceAttributes(service))

        assertEquals(NearbySender(7u, "x"), RelayAdverts.sender(mapOf("v" to "1", "n" to "7", "id" to "x")))
        assertNull(RelayAdverts.sender(mapOf("d" to "1", "id" to "x")))
        assertNull(RelayAdverts.sender(mapOf("n" to "300", "id" to "x")))
        assertEquals("x", RelayAdverts.device(mapOf("d" to "1", "id" to "x")))
        assertNull(RelayAdverts.device(mapOf("n" to "7", "id" to "x")))
        assertEquals(mapOf("id" to "x", "n" to null), RelayAdverts.text(mapOf("id" to "x".toByteArray(), "n" to null)))
    }

    @Test
    fun aCodeIsAdvertisedOnlyWhileItCanBeUsed() {
        assertEquals(
            CodeAdvert(3u, 9u, "id"),
            RelayModel.advert(CodeSend.Waiting("3-a-b-c", 3u, 9u, "id", "f", 1u, null)),
        )
        assertNull(RelayModel.advert(CodeSend.Sent("f", 1u, 1.0, 1.0)))
        assertEquals(true, RelayModel.isRunning(CodeSend.Preparing("f", 0u)))
    }

    @Test
    fun onlyFinishedArrivalsAreFelt() {
        fun received(id: ULong, status: TransferStatus) = Transfer(
            id = id, peerId = "p", peerName = "P", direction = Direction.RECEIVED,
            item = Item.Note("hi"), status = status, atMs = 0,
        )
        val running = listOf(received(1u, TransferStatus.InProgress(1u)))
        assertEquals(Feedback.SUCCESS, RelayModel.arrival(running, listOf(received(1u, TransferStatus.Done))))
        assertEquals(Feedback.FAILURE, RelayModel.arrival(running, listOf(received(1u, TransferStatus.Failed("x")))))
        val done = listOf(received(1u, TransferStatus.Done))
        assertNull(RelayModel.arrival(done, done))
    }

    @Test
    fun pickedNamesStayOneFile() {
        assertEquals("a.txt", PickedFiles.safeName("dir/a.txt"))
        assertEquals("env", PickedFiles.safeName(".env"))
        assertEquals("item", PickedFiles.safeName(" "))
    }

    @Test
    fun twoChildrenWithOneSafeNameBothKeepTheirFile() {
        val taken = mutableSetOf<String>()
        assertEquals("env", PickedFiles.distinctName(PickedFiles.safeName(".env"), taken))
        assertEquals("env 2", PickedFiles.distinctName(PickedFiles.safeName("env"), taken))
        assertEquals("a.txt", PickedFiles.distinctName("a.txt", taken))
        assertEquals("a 2.txt", PickedFiles.distinctName("a.txt", taken))
        assertEquals("a 3.txt", PickedFiles.distinctName("a.txt", taken))
    }
}
