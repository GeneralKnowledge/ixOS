// Relay's Rust core as the Android host sees it, and how the host opens it: the Android form of
// the iPhone app's RelayHost.swift.
package org.forgemobile.relay

import org.forgemobile.relay.host.ForgeCore
import org.forgemobile.relay.host.ForgeEnvironment
import uniffi.forge_app.AppPhase
import uniffi.forge_app.LaunchReport
import uniffi.forge_app.RuntimeEvents
import uniffi.relay_core.RelayCore
import uniffi.relay_core.RelayException

/** Relay's Rust core, as the host sees it. */
class RelayHostCore(val core: RelayCore) : ForgeCore {
    override fun setPhase(phase: AppPhase) = core.setPhase(phase)
    override fun events(): RuntimeEvents? = core.events()
    override fun revision(): ULong = core.revision()
    override fun backgroundWorkPending(): Boolean = core.backgroundWorkPending()
    override fun backgroundTimeExpired() = core.backgroundTimeExpired()
    override fun launchReport(): LaunchReport = core.launchReport()
    override fun shutdown() = core.shutdown()
    override fun close() = core.destroy()
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
        return RelayHostCore(openWaitingForClose(env.dataDirectory, deviceName(), offline))
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
