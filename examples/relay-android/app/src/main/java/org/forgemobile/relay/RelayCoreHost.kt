// Relay's Rust core as the Android host sees it, and how the host opens it: the Android form of
// the iPhone app's RelayHost.swift.
package org.forgemobile.relay

import org.forgemobile.relay.host.ForgeCore
import org.forgemobile.relay.host.ForgeEnvironment
import uniffi.forge_app.AppPhase
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvents
import java.io.File
import uniffi.relay_core.ChatCore
import uniffi.relay_core.ChatException
import uniffi.relay_core.RelayCore
import uniffi.relay_core.RelayException

/**
 * Relay's Rust cores, as the host sees them: Relay's own, and the conversations (chat-core,
 * which lists each trusted device as a conversation). The iPhone app gives each a ForgeHost;
 * here they open and close together, so the Chats tab never outlives the devices it talks to.
 * Relay's runtime events drive the host; Chats has its own event stream (`chatEvents`).
 */
class RelayHostCore(
    val core: RelayCore,
    /** Null when the conversations couldn't open ([chatProblem] says why); Devices still work. */
    val chat: ChatCore?,
    val chatProblem: String? = null,
) : ForgeCore {
    override fun setPhase(phase: AppPhase) {
        core.setPhase(phase)
        chat?.setPhase(phase)
    }
    override fun events(): RuntimeEvents? = core.events()
    override fun revision(): ULong = core.revision()
    override fun backgroundWorkPending(): Boolean =
        core.backgroundWorkPending() || chat?.backgroundWorkPending() == true
    override fun backgroundTimeExpired() {
        chat?.backgroundTimeExpired()
        core.backgroundTimeExpired()
    }
    override fun launchReport(): LaunchReport = core.launchReport()
    override fun shutdown() {
        chat?.shutdown()
        core.shutdown()
    }
    override fun close() {
        chat?.destroy()
        core.destroy()
    }
}

/**
 * Opens Relay's core in [ForgeEnvironment.dataDirectory]. The first open names this device
 * [deviceName]; the person can rename it. [offline] (tests and throwaway data) gives a device
 * that never chose no servers, so nothing reaches Relay's real ones.
 *
 * The device key stays in `device.key` in the app's private files for now (stage 4 moves it
 * into the Android Keystore, docs/relay-android.md); the manifest turns backups off, so it
 * never leaves the phone.
 */
class RelayOpener(private val deviceName: () -> String) {
    fun open(env: ForgeEnvironment, offline: Boolean): RelayHostCore {
        // Received files stay in Relay's own folder (`codes/received`); the history opens them.
        val relay = openWaitingForClose(env.dataDirectory, deviceName(), offline)
        // The conversations beside it (`<app>/chat`), inside the folder the FileProvider shares,
        // so attachments open in other apps. The chat server account stays in memory until
        // Android has a secure store for it (stage 3); device conversations need none.
        val chatDirectory = File(File(env.dataDirectory).parentFile, "chat").apply { mkdirs() }
        return try {
            RelayHostCore(relay, openChatWaitingForClose(chatDirectory.absolutePath))
        } catch (e: ChatException) {
            RelayHostCore(relay, null, RelayErrors.describeChat(e))
        }
    }

    private fun openChatWaitingForClose(directory: String): ChatCore {
        val deadline = System.nanoTime() + CLOSE_WAIT_MS * 1_000_000
        while (true) {
            try {
                return ChatCore.withMemoryCredentials(directory)
            } catch (e: ChatException.InUse) {
                if (System.nanoTime() >= deadline) throw e
                Thread.sleep(50)
            }
        }
    }

    /**
     * A core this process just closed keeps its folder's lock until the last call still running
     * on it returns, so a reopen straight after waits a moment for it. Another copy of the app
     * holding the folder still fails, after [CLOSE_WAIT_MS].
     */
    private fun openWaitingForClose(directory: String, name: String, offline: Boolean): RelayCore {
        val deadline = System.nanoTime() + CLOSE_WAIT_MS * 1_000_000
        while (true) {
            try {
                return if (offline) {
                    RelayCore.withoutDefaultServers(directory, name)
                } else {
                    RelayCore(directory, name)
                }
            } catch (e: RelayException.InUse) {
                if (System.nanoTime() >= deadline) throw e
                Thread.sleep(50)
            }
        }
    }

    private companion object {
        const val CLOSE_WAIT_MS = 2_000L
    }
}
