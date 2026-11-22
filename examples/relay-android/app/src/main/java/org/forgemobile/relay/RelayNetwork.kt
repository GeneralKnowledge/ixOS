// Finding Relay devices on the local network with Android's NsdManager (DNS-SD over mDNS): word
// code senders and trusted devices advertise their iroh port, and this finds them for Rust,
// which owns every connection. This only finds addresses. The iPhone app's RelayNetwork.swift
// does the same with Bonjour, with the same service type and TXT keys (RelayAdverts).
package org.forgemobile.relay

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Handler
import android.os.Looper
import android.util.Log
import kotlinx.coroutines.delay
import uniffi.relay_core.DeviceService
import uniffi.relay_core.NearbyDevice
import uniffi.relay_core.NearbySender

class RelayNetwork(context: Context) {
    private val nsd = context.getSystemService(NsdManager::class.java)
    private val main = Handler(Looper.getMainLooper())

    /** An advert seen on the network, resolved to its TXT record and a "host:port". */
    private data class Seen(val txt: Map<String, String?>, val address: String?)

    // Advertising

    private var codeRegistration: NsdManager.RegistrationListener? = null
    private var codeAdvertised: CodeAdvert? = null
    private var deviceRegistration: NsdManager.RegistrationListener? = null
    private var deviceAdvertised: DeviceService? = null

    /** Advertises a code sender on Rust's port, replacing the last advert; null withdraws it. */
    fun advertiseCode(advert: CodeAdvert?) {
        if (advert == codeAdvertised) return
        codeRegistration?.let(::unregister)
        codeRegistration = null
        codeAdvertised = advert
        if (advert == null) return
        codeRegistration = register(
            RelayAdverts.codeName(advert),
            advert.port.toInt(),
            RelayAdverts.codeAttributes(advert),
        )
    }

    /** Publishes the code advert again, after the app comes back from the background. */
    fun readvertiseCode(advert: CodeAdvert?) {
        codeRegistration?.let(::unregister)
        codeRegistration = null
        codeAdvertised = null
        advertiseCode(advert)
    }

    /** Advertises this device's iroh endpoint for trusted devices; null withdraws it. */
    fun advertiseDevice(service: DeviceService?) {
        if (service == deviceAdvertised) return
        deviceRegistration?.let(::unregister)
        deviceRegistration = null
        deviceAdvertised = service
        if (service == null) return
        deviceRegistration = register(
            RelayAdverts.deviceName(service),
            service.port.toInt(),
            RelayAdverts.deviceAttributes(service),
        )
    }

    private fun register(name: String, port: Int, attributes: Map<String, String>): NsdManager.RegistrationListener {
        val info = NsdServiceInfo().apply {
            serviceName = name
            serviceType = RelayAdverts.SERVICE_TYPE
            setPort(port)
            attributes.forEach { (key, value) -> setAttribute(key, value) }
        }
        val listener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) = Unit
            override fun onRegistrationFailed(info: NsdServiceInfo, error: Int) {
                Log.w(TAG, "advert failed ($error)")
                // Forget it, so the next call with the same advert registers again. Only this
                // registration's own record: a newer advert may have replaced it.
                main.post {
                    if (codeRegistration === this) {
                        codeRegistration = null
                        codeAdvertised = null
                    }
                    if (deviceRegistration === this) {
                        deviceRegistration = null
                        deviceAdvertised = null
                    }
                }
            }
            override fun onServiceUnregistered(info: NsdServiceInfo) = Unit
            override fun onUnregistrationFailed(info: NsdServiceInfo, error: Int) = Unit
        }
        try {
            nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "advert refused: $e")
        }
        return listener
    }

    private fun unregister(listener: NsdManager.RegistrationListener) {
        try {
            nsd.unregisterService(listener)
        } catch (e: IllegalArgumentException) {
            // Never registered (it failed), so nothing to withdraw.
        }
    }

    // Browsing: one discovery while anything watches, shared by every watcher.

    private val seen = mutableMapOf<String, Seen>()
    private val watchers = mutableMapOf<Any, () -> Unit>()
    private var discovery: NsdManager.DiscoveryListener? = null

    /** Services found and not lost since: a resolve that ends after its service went is dropped. */
    private val present = mutableSetOf<String>()

    private fun watch(key: Any, changed: () -> Unit) {
        watchers[key] = changed
        if (discovery == null) startDiscovery()
        changed()
    }

    private fun unwatch(key: Any) {
        watchers.remove(key)
        if (watchers.isEmpty()) stopDiscovery()
    }

    private fun changed() {
        watchers.values.toList().forEach { it() }
    }

    private fun startDiscovery() {
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(type: String) = Unit
            override fun onDiscoveryStopped(type: String) = Unit
            override fun onStartDiscoveryFailed(type: String, error: Int) {
                Log.w(TAG, "discovery failed ($error)")
                // Try again shortly while anything still watches, unless discovery was
                // stopped or started afresh meanwhile.
                val failed = this
                main.post {
                    if (discovery !== failed) return@post
                    discovery = null
                    main.postDelayed({
                        if (discovery == null && watchers.isNotEmpty()) startDiscovery()
                    }, 2_000)
                }
            }
            override fun onStopDiscoveryFailed(type: String, error: Int) = Unit
            override fun onServiceFound(info: NsdServiceInfo) {
                main.post {
                    if (discovery == null) return@post
                    present += info.serviceName
                    resolveLater(info)
                }
            }
            override fun onServiceLost(info: NsdServiceInfo) {
                main.post {
                    present -= info.serviceName
                    queue.removeAll { it.serviceName == info.serviceName }
                    if (seen.remove(info.serviceName) != null) changed()
                }
            }
        }
        discovery = listener
        try {
            nsd.discoverServices(RelayAdverts.SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "discovery refused: $e")
        }
    }

    private fun stopDiscovery() {
        val listener = discovery ?: return
        discovery = null
        try {
            nsd.stopServiceDiscovery(listener)
        } catch (e: IllegalArgumentException) {
            // Discovery never started.
        }
        seen.clear()
        present.clear()
        queue.clear()
    }

    // NsdManager resolves one service at a time (before Android 14), so resolves queue.
    private val queue = ArrayDeque<NsdServiceInfo>()
    private var resolving = false

    private fun resolveLater(info: NsdServiceInfo) {
        if (discovery == null) return
        if (queue.none { it.serviceName == info.serviceName }) queue.addLast(info)
        resolveNext()
    }

    @Suppress("DEPRECATION")
    private fun resolveNext() {
        if (resolving || discovery == null) return
        val info = queue.removeFirstOrNull() ?: return
        resolving = true
        nsd.resolveService(info, object : NsdManager.ResolveListener {
            override fun onResolveFailed(info: NsdServiceInfo, error: Int) {
                main.post {
                    resolving = false
                    if (error == NsdManager.FAILURE_ALREADY_ACTIVE && info.serviceName in present) {
                        // Another resolve (another app's) is running: try again shortly.
                        main.postDelayed({ resolveLater(info) }, 200)
                    }
                    resolveNext()
                }
            }

            override fun onServiceResolved(info: NsdServiceInfo) {
                val txt = RelayAdverts.text(info.attributes)
                // NsdManager resolves an IPv4 address where the advert has one.
                val address = info.host?.hostAddress?.let { RelayFormat.address(it, info.port) }
                main.post {
                    resolving = false
                    if (discovery != null && info.serviceName in present) {
                        seen[info.serviceName] = Seen(txt, address)
                        changed()
                    }
                    resolveNext()
                }
            }
        })
    }

    // What the screens ask for

    /**
     * Reports every code sender advertising nearby (this device's own included; Rust leaves its
     * own id out) until [stopWatchingCodeSenders].
     */
    fun watchCodeSenders(report: (List<NearbySender>) -> Unit) {
        watch(CODE_WATCHER) { report(seen.values.mapNotNull { RelayAdverts.sender(it.txt) }) }
    }

    fun stopWatchingCodeSenders() = unwatch(CODE_WATCHER)

    /** Reports the devices advertising `d=1` nearby until [stopWatchingDevices]. */
    fun watchDevices(report: (List<NearbyDevice>) -> Unit) {
        watch(DEVICE_WATCHER) {
            report(
                seen.values.mapNotNull { advert ->
                    val id = RelayAdverts.device(advert.txt) ?: return@mapNotNull null
                    val address = advert.address ?: return@mapNotNull null
                    NearbyDevice(id, listOf(address))
                },
            )
        }
    }

    fun stopWatchingDevices() = unwatch(DEVICE_WATCHER)

    /**
     * Looks for a code sender whose TXT record says code number [number], for up to 10 s, then
     * 1.5 s more for a second one. Called on the main thread.
     */
    suspend fun findCodeSender(number: UByte): CodeSender {
        val key = Any()
        var matches: Map<String, String?> = emptyMap()
        watch(key) {
            matches = seen.values.mapNotNull { advert ->
                val sender = RelayAdverts.sender(advert.txt) ?: return@mapNotNull null
                if (sender.number != number) null else sender.endpointId to advert.address
            }.toMap()
        }
        try {
            var waited = 0
            var firstSeenAt: Int? = null
            while (waited < 100) {
                delay(100)
                waited += 1
                if (matches.isNotEmpty() && firstSeenAt == null) firstSeenAt = waited
                val first = firstSeenAt
                if (first != null && waited - first >= 15) break
            }
        } finally {
            unwatch(key)
        }
        if (matches.size > 1) return CodeSender.Several
        val (id, address) = matches.entries.firstOrNull() ?: return CodeSender.None
        return if (address == null) CodeSender.None else CodeSender.Found(listOf(address), id)
    }

    /** Withdraws every advert and stops looking. */
    fun close() {
        advertiseCode(null)
        advertiseDevice(null)
        watchers.clear()
        stopDiscovery()
    }

    private companion object {
        const val TAG = "RelayNetwork"
        val CODE_WATCHER = Any()
        val DEVICE_WATCHER = Any()
    }
}
