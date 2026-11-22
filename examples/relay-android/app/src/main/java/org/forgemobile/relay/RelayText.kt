// Relay's words on Android: the same as the iPhone app's (RelayModel.swift's "Words for the
// screen" and RelayFormat, TrustedDeviceViews.swift). Plain Kotlin, so JVM tests check them.
package org.forgemobile.relay

import java.text.DateFormat
import java.util.Date
import java.util.Locale
import org.forgemobile.relay.kit.ForgeErrorDescriber
import org.forgemobile.relay.kit.ForgeFailure
import uniffi.relay_core.CodeReceipt
import uniffi.relay_core.ConnectionMode
import uniffi.relay_core.Device
import uniffi.relay_core.DevicesSynced
import uniffi.relay_core.Direction
import uniffi.relay_core.Item
import uniffi.relay_core.PairingPolicy
import uniffi.relay_core.Presence
import uniffi.relay_core.RelayException
import uniffi.relay_core.RelayServers
import uniffi.relay_core.Transfer
import uniffi.relay_core.TransferStatus
import uniffi.relay_core.ownRelayServers

/** Gives Relay's Rust errors their words, wherever the kit shows a failure. */
object RelayErrors : ForgeErrorDescriber {
    override fun failure(error: Throwable): ForgeFailure? {
        if (error !is RelayException) return null
        val retryable = error is RelayException.Unreachable ||
            error is RelayException.Connection ||
            error is RelayException.Storage
        return ForgeFailure(title(error), describe(error), retryable)
    }

    override fun isCancellation(error: Throwable): Boolean = error is RelayException.Stopped

    fun describe(error: RelayException): String = when (error) {
        is RelayException.EmptyName -> "Give this device a name."
        is RelayException.NameTooLong -> "Names can be up to ${error.max} characters."
        is RelayException.EmptyNote -> "Write something to send."
        is RelayException.NoteTooLong -> "Notes can be up to ${error.max} characters."
        is RelayException.FileTooLarge -> "Files can be up to 2 GB."
        is RelayException.WordCode -> sentence(error.reason)
        is RelayException.UnknownPeer -> "This device doesn't trust that one any more."
        is RelayException.NotFound -> "That's no longer in the history."
        is RelayException.Unreachable ->
            "Can't reach the other device. Check that it has Relay open, on the same Wi-Fi."
        is RelayException.Refused -> "The other device didn't accept this one. Send with a code instead."
        is RelayException.Declined -> "The other device declined: ${error.reason}."
        is RelayException.Connection -> "The connection broke. Try again."
        is RelayException.File -> "The file couldn't be read or kept."
        is RelayException.InUse -> "Relay is open in another copy of the app."
        is RelayException.Corrupt -> "Relay's saved data couldn't be read."
        is RelayException.NewerData -> "Relay's data was saved by a newer version of the app."
        is RelayException.Storage -> "Couldn't save. Try again."
        is RelayException.Stopped -> "Stopped."
    }

    fun title(error: RelayException): String = when (error) {
        is RelayException.EmptyName, is RelayException.NameTooLong, is RelayException.EmptyNote,
        is RelayException.NoteTooLong, is RelayException.FileTooLarge -> "Check the Details"
        is RelayException.Refused -> "Not Accepted"
        is RelayException.WordCode -> "Check the Code"
        is RelayException.UnknownPeer, is RelayException.NotFound -> "Not Found"
        is RelayException.Unreachable, is RelayException.Connection, is RelayException.Declined -> "Not Sent"
        is RelayException.File -> "File Problem"
        is RelayException.InUse -> "Already Open"
        is RelayException.Corrupt, is RelayException.NewerData, is RelayException.Storage -> "Couldn't Save"
        is RelayException.Stopped -> "Stopped"
    }

    /**
     * What a code send or receive says when it fails: the reason Rust gave, as a sentence, for
     * the errors that carry one, and the usual words for the rest.
     */
    fun message(error: Throwable): String = when (error) {
        is RelayException.Declined -> sentence(error.reason)
        is RelayException.Unreachable -> sentence(error.reason)
        is RelayException.Connection -> sentence(error.reason)
        is RelayException.File -> sentence(error.reason)
        is RelayException -> describe(error)
        else -> error.message ?: error.toString()
    }

    /** "the code has expired" → "The code has expired." */
    fun sentence(reason: String): String {
        if (reason.isEmpty()) return reason
        val capital = reason.replaceFirstChar { it.titlecase(Locale.ROOT) }
        return if (capital.endsWith(".")) capital else "$capital."
    }
}

object RelayFormat {
    /** "1.2 MB", in the units the iPhone's file sizes use (1000 bytes to a kB). */
    fun size(bytes: ULong): String {
        val b = bytes.toDouble()
        return when {
            b < 1_000 -> "${bytes} bytes"
            b < 1_000_000 -> oneDecimal(b / 1_000, "KB")
            b < 1_000_000_000 -> oneDecimal(b / 1_000_000, "MB")
            else -> String.format(Locale.ROOT, "%.2f GB", b / 1_000_000_000)
        }
    }

    private fun oneDecimal(value: Double, unit: String): String =
        if (value >= 100) "${value.toLong()} $unit" else String.format(Locale.ROOT, "%.1f %s", value, unit)

    /** What a history row says: the note, or the file's name. */
    fun title(item: Item): String = when (item) {
        is Item.Note -> item.text
        is Item.File -> item.name
    }

    /** "Waiting for MacBook", "Sending 40%", "Sent", "Received", "Failed: …". */
    fun status(transfer: Transfer): String = when (val status = transfer.status) {
        is TransferStatus.Waiting -> "Waiting for ${transfer.peerName}"
        is TransferStatus.InMailbox -> "In the Mailbox"
        is TransferStatus.InProgress -> {
            val verb = if (transfer.direction == Direction.SENT) "Sending" else "Receiving"
            val item = transfer.item
            if (item is Item.File && item.size > 0u) {
                "$verb ${(status.done.toDouble() * 100 / item.size.toDouble()).toInt()}%"
            } else {
                "$verb…"
            }
        }
        is TransferStatus.Done -> if (transfer.direction == Direction.SENT) "Sent" else "Received"
        is TransferStatus.Failed -> "Failed: ${status.reason}"
    }

    /** "To MacBook" or "From MacBook". */
    fun peer(transfer: Transfer): String =
        if (transfer.direction == Direction.SENT) "To ${transfer.peerName}" else "From ${transfer.peerName}"

    fun time(ms: Long): String = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ms))

    /** Where to reach a device found on the network, as Rust takes it. */
    fun address(host: String, port: Int): String {
        // An IPv6 host may carry a scope ("fe80::1%wlan0"), which Rust can't use.
        val bare = host.substringBefore('%')
        return if (bare.contains(':')) "[$bare]:$port" else "$bare:$port"
    }

    /** "Received photo.jpg, 2.1 MB in 1.4 s (1.5 MB/s) from Pixel." */
    fun receipt(receipt: CodeReceipt): String {
        val resumed = if (receipt.resumedFrom > 0u) ", resumed from ${size(receipt.resumedFrom)}" else ""
        return "Received ${receipt.name}, ${size(receipt.size)} in " +
            String.format(Locale.ROOT, "%.1f s (%.1f MB/s)", receipt.seconds, receipt.megabytesPerSecond) +
            "$resumed from ${receipt.senderName}."
    }

    /** "Nearby", "Reachable, seen 2 min ago" or "Offline, seen 3 h ago". */
    fun whereNow(device: Device, nowMs: Long = System.currentTimeMillis()): String {
        val word = when (device.presence) {
            Presence.NEARBY -> return "Nearby"
            Presence.REACHABLE -> "Reachable"
            Presence.OFFLINE -> "Offline"
        }
        val ms = device.lastSeenMs ?: return "$word, not reached yet"
        val seconds = maxOf(0L, (nowMs - ms) / 1000)
        val ago = when {
            seconds < 60 -> "just now"
            seconds < 3_600 -> "${seconds / 60} min ago"
            seconds < 172_800 -> "${seconds / 3_600} h ago"
            else -> "${seconds / 86_400} days ago"
        }
        return "$word, seen $ago"
    }

    /** How a trusted device can be reached, in words. */
    fun reach(device: Device): String {
        if (device.nearby) return "${device.name} is nearby. No code needed."
        device.problem?.let { return it }
        if (device.presence == Presence.REACHABLE) return "${device.name} can be reached. No code needed."
        device.route?.let { return "Last connected: $it. Relay tries it there again when something is waiting." }
        return "${device.name} isn't showing on this network. Open Relay on it, on the same Wi-Fi."
    }

    /** The name a row shows: "Sam (Pixel)" for a person's device. */
    fun rowName(device: Device): String = device.person?.let { "$it (${device.name})" } ?: device.name

    fun platformName(os: String): String = when (os) {
        "ios" -> "iPhone or iPad"
        "macos" -> "Mac"
        "linux" -> "Linux"
        "windows" -> "Windows"
        "android" -> "Android"
        else -> os
    }

    fun revoked(device: Device): String =
        device.revokedBy?.let { "Revoked by $it: can't send here without a code" }
            ?: "Revoked: can't send here without a code"

    fun synced(synced: DevicesSynced, from: String): String {
        val parts = buildList {
            if (synced.offered > 0u) add(if (synced.offered == 1u) "1 device to trust" else "${synced.offered} devices to trust")
            if (synced.revoked > 0u) add(if (synced.revoked == 1u) "1 device revoked" else "${synced.revoked} devices revoked")
        }
        return if (parts.isEmpty()) "Nothing new from $from." else "From $from: ${parts.joinToString(", ")}."
    }

    fun policyName(policy: PairingPolicy): String = when (policy) {
        PairingPolicy.ASK_TO_PAIR -> "Ask to Trust"
        PairingPolicy.PAIR_AUTOMATICALLY -> "Trust Again Automatically"
        PairingPolicy.CODES_ONLY -> "Codes Only"
    }

    fun explain(policy: PairingPolicy): String = when (policy) {
        PairingPolicy.ASK_TO_PAIR ->
            "After a code transfer, Relay asks whether to trust the other device, so you can send to each other without a code."
        PairingPolicy.PAIR_AUTOMATICALLY ->
            "A device you trusted before, then forgot, is trusted again after a code transfer. New devices are asked about. A revoked device never is."
        PairingPolicy.CODES_ONLY -> "A code moves one transfer and nothing is remembered."
    }

    fun modeName(mode: ConnectionMode): String = when (mode) {
        ConnectionMode.HYBRID -> "Everywhere"
        ConnectionMode.PRIVATE -> "Private Networks"
        ConnectionMode.LOCAL -> "This Network Only"
    }

    /** What a Connections choice means, claiming nothing more than it does. */
    fun explain(mode: ConnectionMode, servers: RelayServers): String = when (mode) {
        ConnectionMode.HYBRID -> when (ServerChoice.of(servers)) {
            ServerChoice.RELAYS ->
                "Your devices reach each other from anywhere. When there's no direct way, an encrypted connection passes through Relay's server, which can't read it but sees that two devices are connected, when and how much."
            ServerChoice.OWN ->
                "Your devices reach each other from anywhere. When there's no direct way, an encrypted connection passes through your own server, which can't read it but sees that two devices are connected, when and how much."
            ServerChoice.NONE ->
                "Your devices reach each other directly when they can, over Wi-Fi, a private network or the internet. With no server, some networks keep them apart."
        }
        ConnectionMode.PRIVATE ->
            "Only the same Wi-Fi or a private network such as Tailscale. Nothing goes through Relay's servers."
        ConnectionMode.LOCAL -> "Only the same Wi-Fi or wired network."
    }
}

/** Which servers Everywhere uses, as the Servers screen offers them. */
enum class ServerChoice(val title: String) {
    RELAYS("Relay's Servers"),
    OWN("Your Own Servers"),
    NONE("No Servers");

    val shortName: String
        get() = when (this) {
            RELAYS -> "Relay's"
            OWN -> "Your Own"
            NONE -> "None"
        }

    val explanation: String
        get() = when (this) {
            RELAYS ->
                "relay.avedeus.ovh passes on what no direct way reaches, and lookup.avedeus.ovh tells your trusted devices where to find this one. Both see that your devices connect, never what they send."
            OWN ->
                "A relay and lookup server you run (iroh-relay and a pkarr server). The command line sets them with relay servers."
            NONE -> "Your devices reach each other only when one can connect straight to the other."
        }

    companion object {
        fun of(servers: RelayServers, relays: RelayServers = ownRelayServers()): ServerChoice = when {
            servers.relayUrl == null && servers.lookupUrl == null -> NONE
            servers == relays -> RELAYS
            else -> OWN
        }

        /** The servers typed in, blank fields left out. */
        fun own(relayUrl: String, lookupUrl: String): RelayServers =
            RelayServers(relayUrl.trim().ifEmpty { null }, lookupUrl.trim().ifEmpty { null })
    }
}
