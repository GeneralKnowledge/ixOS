// What Relay advertises on the local network and how it reads other devices' adverts: the same
// DNS-SD service and TXT keys as the iPhone app (RelayNetwork.swift) and the `relay` command, so
// an Android phone, an iPhone and a computer find each other. Plain Kotlin, tested on the JVM.
package org.forgemobile.relay

import uniffi.relay_core.DeviceService
import uniffi.relay_core.NearbySender

object RelayAdverts {
    /** The service type every Relay advert uses: code senders and trusted devices alike. */
    const val SERVICE_TYPE = "_forgerelay._udp"

    /** A code sender's advert (TXT `v`, `n`, `id`, as the M0 spike does). */
    fun codeName(advert: CodeAdvert): String = "relay-${advert.endpointId.take(8)}-${advert.number}"

    fun codeAttributes(advert: CodeAdvert): Map<String, String> = mapOf(
        "v" to "1",
        "n" to advert.number.toString(),
        "id" to advert.endpointId,
    )

    /** This device's advert for trusted devices (TXT `v=1`, `d=1`, `id`; no `n`). */
    fun deviceName(service: DeviceService): String = "relay-${service.endpointId.take(12)}"

    fun deviceAttributes(service: DeviceService): Map<String, String> = mapOf(
        "v" to "1",
        "d" to "1",
        "id" to service.endpointId,
    )

    /** A code sender, from an advert's TXT record: null when it isn't one. */
    fun sender(txt: Map<String, String?>): NearbySender? {
        val number = txt["n"]?.toIntOrNull()?.takeIf { it in 0..255 } ?: return null
        val id = txt["id"]?.takeIf { it.isNotEmpty() } ?: return null
        return NearbySender(number.toUByte(), id)
    }

    /** A trusted-device advert's endpoint id, from its TXT record: null when it isn't one. */
    fun device(txt: Map<String, String?>): String? {
        if (txt["d"] != "1") return null
        return txt["id"]?.takeIf { it.isNotEmpty() }
    }

    /** Android hands TXT values over as bytes. */
    fun text(attributes: Map<String, ByteArray?>): Map<String, String?> =
        attributes.mapValues { (_, value) -> value?.toString(Charsets.UTF_8) }
}
