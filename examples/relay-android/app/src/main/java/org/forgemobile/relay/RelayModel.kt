// Relay's screen state on Android: plain Kotlin and coroutines (no Android UI), so the JVM unit
// tests drive it against the real Rust core. It holds no rules: trust, encryption, sending and
// the history are Relay's Rust core (examples/relay/rust); this asks it and shows what it says,
// like the iPhone app's RelayModel.swift. Finding devices on the network is RelayNetwork, which
// hands this model addresses.
package org.forgemobile.relay

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext
import org.forgemobile.relay.kit.ForgeAlert
import org.forgemobile.relay.kit.ForgeFailure
import uniffi.relay_core.CodeReceipt
import uniffi.relay_core.CodeSend
import uniffi.relay_core.CodeSize
import uniffi.relay_core.Capability
import uniffi.relay_core.ConnectionMode
import uniffi.relay_core.Device
import uniffi.relay_core.DeviceService
import uniffi.relay_core.Direction
import uniffi.relay_core.Identity
import uniffi.relay_core.NearbyDevice
import uniffi.relay_core.NearbySender
import uniffi.relay_core.PairingPolicy
import uniffi.relay_core.RelayCore
import uniffi.relay_core.RelayException
import uniffi.relay_core.RelayServers
import uniffi.relay_core.SavedDataNotice
import uniffi.relay_core.ServerHealth
import uniffi.relay_core.Transfer
import uniffi.relay_core.TransferStatus
import uniffi.relay_core.TrustOffer
import uniffi.relay_core.wordCodeNumber

/** What Relay's Rust core says now: the screens show this. */
data class RelayState(
    val identity: Identity,
    /** Everything sent and received, newest first. */
    val transfers: List<Transfer> = emptyList(),
    /** Trusted and revoked devices. Trust is keyed by endpoint id in Rust, never the name. */
    val devices: List<Device> = emptyList(),
    val policy: PairingPolicy = PairingPolicy.ASK_TO_PAIR,
    val connectionMode: ConnectionMode = ConnectionMode.HYBRID,
    val servers: RelayServers = RelayServers(null, null),
    val serverHealth: ServerHealth = ServerHealth.OFF,
    /** "Trust this device?" after a code transfer, until answered. */
    val trustOffer: TrustOffer? = null,
    /** Saved data Relay couldn't read when it opened, until the person dismisses it. */
    val savedDataNotice: SavedDataNotice? = null,
    /** An upgrade turned Relay's servers on: said once, until dismissed. */
    val awayNotice: String? = null,
) {
    val trustedDevices: List<Device> get() = devices.filter { it.trusted }
    val revokedDevices: List<Device> get() = devices.filter { !it.trusted }
    val isEmpty: Boolean get() = devices.isEmpty() && transfers.isEmpty()

    /** Whether anything is still on its way, so the screen refreshes its progress. */
    val isTransferring: Boolean get() = transfers.any { it.status is TransferStatus.InProgress }

    /** Whether anything waits in the outbox for a trusted device. */
    val isWaiting: Boolean get() = transfers.any { it.status is TransferStatus.Waiting }

    fun device(endpointId: String): Device? = devices.firstOrNull { it.endpointId == endpointId }
}

/** Where a code receive is. */
sealed interface CodeReceive {
    data object Idle : CodeReceive
    /** Looking for the sender on this Wi-Fi. */
    data object Finding : CodeReceive
    /** Connected; [size] once the sender has named the file. */
    data class Receiving(val size: CodeSize?, val sinceMs: Long) : CodeReceive
    data class Received(val receipt: CodeReceipt) : CodeReceive
    data class Failed(val message: String) : CodeReceive
}

/** What the network found for a code's number (RelayNetwork looks). */
sealed interface CodeSender {
    data class Found(val addresses: List<String>, val endpointId: String) : CodeSender
    data object None : CodeSender
    /** More than one sender uses the number: guessing would spend a stranger's code, so stop. */
    data object Several : CodeSender
}

/** What to advertise while there is a send code: its number, Rust's port and endpoint id. */
data class CodeAdvert(val number: UByte, val port: UShort, val endpointId: String)

/** The haptic a screen plays: success when something is done, failure with the reason. */
enum class Feedback { SUCCESS, FAILURE }

class RelayModel(
    val core: RelayCore,
    /** Where Rust's async calls are awaited: a pool, never a dispatcher that resumes in place. */
    private val rust: CoroutineDispatcher = Dispatchers.Default,
) {
    /**
     * Where a send or a code receive runs once started: as long as the model, not the screen, so
     * rotating the phone (which rebuilds the screens) neither drops nor strands it.
     */
    private val work by lazy { CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate) }

    /** Alerts the screens ask for: confirm, or a failure. */
    sealed interface Action {
        data class Remove(val transfer: ULong) : Action
        data class Revoke(val endpointId: String) : Action
        data class Forget(val endpointId: String) : Action
    }

    init {
        ForgeFailure.describer = RelayErrors
    }

    private val _state = MutableStateFlow(RelayState(core.identity()))
    val state: StateFlow<RelayState> = _state.asStateFlow()

    private val _alert = MutableStateFlow<ForgeAlert<Action>?>(null)
    val alert: StateFlow<ForgeAlert<Action>?> = _alert.asStateFlow()

    private val _feedback = MutableSharedFlow<Feedback>(extraBufferCapacity = 8)
    val feedback: SharedFlow<Feedback> = _feedback.asSharedFlow()

    private val _codeReceive = MutableStateFlow<CodeReceive>(CodeReceive.Idle)
    val codeReceive: StateFlow<CodeReceive> = _codeReceive.asStateFlow()

    private val _codeSend = MutableStateFlow<CodeSend>(CodeSend.Idle)
    val codeSend: StateFlow<CodeSend> = _codeSend.asStateFlow()

    init {
        reload(announcing = false)
    }

    /** Re-reads Rust's published state. Cheap: no command is waited for. */
    fun reload() = reload(announcing = true)

    /**
     * [announcing]: whether a transfer that finished arriving since the last read plays its
     * haptic. False when the action that received it plays its own (a code receive).
     */
    private fun reload(announcing: Boolean) {
        val previous = _state.value.transfers
        val transfers = core.transfers()
        _state.value = RelayState(
            identity = core.identity(),
            transfers = transfers,
            devices = core.devices(),
            policy = core.pairingPolicy(),
            connectionMode = core.connectionMode(),
            servers = core.servers(),
            serverHealth = core.serverHealth(),
            trustOffer = core.trustOffer(),
            savedDataNotice = core.savedDataNotice(),
            awayNotice = core.awayFromHomeNotice(),
        )
        if (announcing && !isReceivingCode) arrival(previous, transfers)?.let(::play)
    }

    /** Re-reads the devices (presence ages) and whether the servers answer. */
    fun refreshDevices() {
        _state.update { it.copy(devices = core.devices(), serverHealth = core.serverHealth()) }
    }

    fun refreshServerHealth() {
        val health = core.serverHealth()
        if (health != _state.value.serverHealth) _state.update { it.copy(serverHealth = health) }
    }

    /** Re-reads the trust offer: the sending side learns the other's name after its send ends. */
    fun refreshTrustOffer() {
        _state.update { it.copy(trustOffer = core.trustOffer()) }
    }

    private fun play(kind: Feedback) {
        _feedback.tryEmit(kind)
    }

    fun dismissAlert() {
        _alert.value = null
    }

    /** Reports a failure the screens ran into themselves. */
    fun report(error: Throwable) {
        val alert = ForgeAlert.failure<Action>(error) ?: return
        _alert.value = alert
        play(Feedback.FAILURE)
    }

    /** Shows a failure the screens put in words themselves. */
    fun report(failure: ForgeFailure) {
        _alert.value = ForgeAlert.failure(failure)
        play(Feedback.FAILURE)
    }

    private suspend fun perform(command: suspend (RelayCore) -> Unit) {
        try {
            withContext(rust) { command(core) }
            reload()
            play(Feedback.SUCCESS)
        } catch (e: RelayException) {
            reload()
            report(e)
        }
    }

    // The notices

    suspend fun dismissSavedDataNotice() {
        _state.update { it.copy(savedDataNotice = null) }
        perform { it.dismissSavedDataNotice() }
    }

    suspend fun dismissAwayNotice() {
        _state.update { it.copy(awayNotice = null) }
        perform { it.dismissAwayFromHomeNotice() }
    }

    // This device and the history

    /** Renames this device; throws so the rename dialog can show why. */
    suspend fun rename(name: String) {
        withContext(rust) { core.renameDevice(name) }
        reload()
    }

    /** Runs a confirmed action. */
    suspend fun handle(action: Action) = when (action) {
        is Action.Remove -> perform { it.removeTransfer(action.transfer) }
        is Action.Revoke -> perform { it.revokeDevice(action.endpointId) }
        is Action.Forget -> perform { it.forgetDevice(action.endpointId) }
    }

    suspend fun remove(transfer: Transfer) = perform { it.removeTransfer(transfer.id) }

    /** Stops a send to a trusted device that is on its way, or takes it off the outbox. */
    fun cancel(transfer: Transfer) {
        core.cancelTransfer(transfer.id)
        reload()
    }

    // Trusted devices

    /** Where this device takes trusted connections, to advertise; null if it couldn't start. */
    suspend fun deviceService(): DeviceService? = try {
        withContext(rust) { core.deviceService() }
    } catch (e: RelayException) {
        null
    }

    /** The trusted devices the network sees now, with where they are. */
    fun devicesNearby(nearby: List<NearbyDevice>) {
        core.devicesNearby(nearby)
        _state.update { it.copy(devices = core.devices()) }
    }

    /**
     * Asks a trusted device what it says about itself. Not reaching it is not an error the
     * person needs to see: it just stays as it was.
     */
    suspend fun refreshStatus(device: Device) {
        try {
            withContext(rust) { core.deviceStatus(device.endpointId) }
        } catch (e: RelayException) {
            // Shown on the device's page as its problem, from the next read.
        }
        _state.update { it.copy(devices = core.devices()) }
    }

    /** The person answered Trust. */
    suspend fun trust(offer: TrustOffer) {
        _state.update { it.copy(trustOffer = null) }
        perform { it.trustDevice(offer.endpointId) }
        // The next device waiting to be trusted, if any.
        refreshTrustOffer()
    }

    /** Not Now: nothing is remembered, and the next device waiting, if any, is offered. */
    fun dismissTrustOffer() {
        core.dismissTrustOffer()
        refreshTrustOffer()
    }

    /** Trust Again, for a revoked device: the only way it is trusted again. */
    suspend fun trustAgain(device: Device) = perform { it.trustDevice(device.endpointId) }

    fun askToRevoke(device: Device) {
        _alert.value = ForgeAlert.confirm(
            "Revoke “${device.name}”?",
            message = "It can't send to this device without a code, and a code transfer never trusts it again. Only you can, from this list.",
            confirm = "Revoke",
            action = Action.Revoke(device.endpointId),
            identifier = "trusted.confirmRevoke",
            destructive = true,
        )
    }

    fun askToForget(device: Device) {
        _alert.value = ForgeAlert.confirm(
            "Forget “${device.name}”?",
            message = if (device.trusted) {
                "It leaves this list and can't send without a code. Its history stays."
            } else {
                "It leaves this list and stays revoked. Its history stays."
            },
            confirm = "Forget Device",
            action = Action.Forget(device.endpointId),
            identifier = "trusted.confirmForget",
            destructive = true,
        )
    }

    suspend fun setReceives(on: Boolean, device: Device) =
        perform { it.setReceivesFrom(device.endpointId, on) }

    suspend fun setIntroduces(on: Boolean, device: Device) =
        perform { it.setIntroduces(device.endpointId, on) }

    /** Marks [device] as [person]'s, or (with null) as one of your own devices. */
    suspend fun setPerson(person: String?, device: Device) =
        perform { it.setPerson(device.endpointId, person) }

    suspend fun setSeesDevices(on: Boolean, device: Device) =
        perform { it.setSeesDevices(device.endpointId, on) }

    /** Grants [capability] to [device], or takes it away; taking it away ends any use at once. */
    suspend fun setGrant(capability: Capability, on: Boolean, device: Device) =
        perform { it.setGrant(device.endpointId, capability, on) }

    /**
     * Asks an introducer for its devices now. Returns what changed, in words, or null when it
     * failed (the failure is shown as an alert).
     */
    suspend fun syncDevices(device: Device): String? {
        val said = try {
            val synced = withContext(rust) { core.syncDevices(device.endpointId) }
            reload()
            play(Feedback.SUCCESS)
            RelayFormat.synced(synced, device.name)
        } catch (e: RelayException) {
            // Nothing was being sent, so not the usual "Not Sent".
            _alert.value = ForgeAlert.failure(
                ForgeFailure("Couldn't Check Its Devices", RelayErrors.describe(e), isRetryable = false),
            )
            play(Feedback.FAILURE)
            null
        }
        refreshTrustOffer()
        return said
    }

    suspend fun setPolicy(policy: PairingPolicy) = perform { it.setPairingPolicy(policy) }

    suspend fun setConnectionMode(mode: ConnectionMode) = perform { it.setConnectionMode(mode) }

    /** Takes effect at once, without reopening Relay. */
    suspend fun setServers(servers: RelayServers) = perform { it.setServers(servers) }

    /**
     * Sends files (in one folder when several) to a trusted device. Relay copies them in first,
     * so they can wait in the outbox and the picked copies can go when this returns.
     */
    suspend fun send(paths: List<String>, device: Device): Boolean {
        reload()
        return try {
            val sent = withContext(rust) { core.sendToDevice(device.endpointId, paths) }
            reload()
            // A waiting item is queued, not sent: no success tap yet.
            if (sent.status is TransferStatus.Done) play(Feedback.SUCCESS)
            true
        } catch (e: RelayException) {
            reload()
            if (e !is RelayException.Stopped) report(e)
            false
        }
    }

    /**
     * Sends what the person picked to [device]: [copy] brings the files in, then Relay sends
     * them, and [finished] runs either way (to remove the copies). Outlives the screen.
     */
    fun sendPicked(device: Device, copy: suspend () -> List<String>, finished: (List<String>) -> Unit) {
        work.launch {
            var paths = emptyList<String>()
            try {
                paths = copy()
                send(paths, device)
            } catch (e: java.io.IOException) {
                report(ForgeFailure("Couldn't Read the File", e.message ?: "$e"))
            } finally {
                finished(paths)
            }
        }
    }

    // Receiving with a word code

    val isReceivingCode: Boolean
        get() = _codeReceive.value.let { it is CodeReceive.Finding || it is CodeReceive.Receiving }

    /** The code receive in hand: a stopped or older one finding its sender afterwards is dropped. */
    private var receiving = 0L

    /** [receive], as long as the model rather than the screen. */
    fun startReceive(code: String, find: suspend (UByte) -> CodeSender) {
        work.launch { receive(code, find) }
    }

    /** Finds the sender of [code] with [find] (the network, by the code's number), then receives. */
    suspend fun receive(code: String, find: suspend (UByte) -> CodeSender) {
        val number = try {
            wordCodeNumber(code)
        } catch (e: RelayException) {
            _codeReceive.value = CodeReceive.Failed(RelayErrors.message(e))
            return
        }
        val attempt = ++receiving
        _codeReceive.value = CodeReceive.Finding
        val sender = try {
            find(number)
        } catch (e: CancellationException) {
            if (attempt == receiving && _codeReceive.value == CodeReceive.Finding) _codeReceive.value = CodeReceive.Idle
            throw e
        }
        // Stopped while looking: nothing more.
        if (attempt != receiving || _codeReceive.value != CodeReceive.Finding) return
        when (sender) {
            CodeSender.None -> _codeReceive.value = CodeReceive.Failed(
                "No sender with code number $number on this Wi-Fi. Check the sender is still waiting, on the same network.",
            )
            CodeSender.Several -> _codeReceive.value = CodeReceive.Failed(
                "Two senders nearby use number $number. Ask the sender for a new code.",
            )
            is CodeSender.Found -> {
                _codeReceive.value = CodeReceive.Receiving(null, System.currentTimeMillis())
                try {
                    val receipt = withContext(rust) { core.receiveWithCode(code, sender.addresses, sender.endpointId) }
                    reload(announcing = false)
                    _codeReceive.value = CodeReceive.Received(receipt)
                    play(Feedback.SUCCESS)
                    refreshTrustOffer()
                } catch (e: RelayException) {
                    _codeReceive.value = CodeReceive.Failed(RelayErrors.message(e))
                    play(Feedback.FAILURE)
                } catch (e: CancellationException) {
                    // Waiting on Rust was given up: the receive is stopped with it.
                    core.cancelCodeReceive()
                    _codeReceive.value = CodeReceive.Idle
                    throw e
                }
            }
        }
    }

    /** Re-reads the size of the file on its way. */
    fun refreshCodeReceive() {
        val now = _codeReceive.value
        if (now is CodeReceive.Receiving) _codeReceive.value = now.copy(size = core.codeReceiveSize())
    }

    /** Stop: while looking for the sender, stops looking; while receiving, Rust stops. */
    fun cancelCodeReceive() {
        if (_codeReceive.value == CodeReceive.Finding) {
            receiving++
            _codeReceive.value = CodeReceive.Idle
        } else {
            core.cancelCodeReceive()
        }
    }

    fun resetCodeReceive() {
        if (!isReceivingCode) _codeReceive.value = CodeReceive.Idle
    }

    // Sending with a word code

    val isSendingCode: Boolean get() = isRunning(_codeSend.value)

    val codeAdvert: CodeAdvert? get() = advert(_codeSend.value)

    /** Starts sending the files at [paths] with one new code (one folder when several). */
    fun send(paths: List<String>) {
        _codeSend.value = try {
            core.sendPathsWithCode(paths)
            core.codeSendStatus()
        } catch (e: RelayException) {
            CodeSend.Failed(RelayErrors.message(e))
        }
    }

    /** The picked files being copied in before Rust has them (0: none); each pick its own number. */
    private var staging = 0L
    private var picks = 0L

    /**
     * Sends with a code what the person picked: shows [name] as preparing while [copy] brings the
     * files in, then Rust sends them. Stop while copying drops them; a newer pick replaces this
     * one. Outlives the screen.
     */
    fun sendPicked(name: String, copy: suspend () -> List<String>) {
        val pick = ++picks
        staging = pick
        _codeSend.value = CodeSend.Preparing(name, 0u)
        work.launch {
            val paths = try {
                copy()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (staging == pick) {
                    staging = 0
                    _codeSend.value = CodeSend.Failed("Couldn't read what you chose: ${e.message ?: e}")
                }
                return@launch
            }
            if (staging != pick) return@launch
            staging = 0
            send(paths)
        }
    }

    fun refreshCodeSend() {
        // Still copying the picked files in: Rust hasn't been asked yet, and what it says is
        // about the send before.
        if (staging != 0L) return
        val now = core.codeSendStatus()
        _codeSend.value = now
        refreshTrustOffer()
        if (now is CodeSend.Sent) reload(announcing = false)
    }

    /** Other code senders the network sees, so Rust avoids their numbers. */
    fun codeSendersNearby(senders: List<NearbySender>) = core.codeSendNearby(senders)

    /** Stop: while the picked files copy in, drops them; once Rust sends, Rust stops. */
    fun cancelCodeSend() {
        if (staging != 0L) {
            staging = 0
            _codeSend.value = CodeSend.Idle
        } else {
            core.cancelCodeSend()
        }
    }

    /** When the screen opens: picks up a send still running in Rust, otherwise starts clean. */
    fun resetCodeSend() {
        val now = core.codeSendStatus()
        if (isRunning(now)) {
            _codeSend.value = now
        } else if (!isSendingCode) {
            _codeSend.value = CodeSend.Idle
        }
    }

    companion object {
        fun isRunning(send: CodeSend): Boolean =
            send is CodeSend.Preparing || send is CodeSend.Waiting || send is CodeSend.Sending

        fun advert(send: CodeSend): CodeAdvert? = when (send) {
            is CodeSend.Waiting -> CodeAdvert(send.number, send.port, send.endpointId)
            is CodeSend.Sending -> CodeAdvert(send.number, send.port, send.endpointId)
            else -> null
        }

        /**
         * The haptic for what arrived between two reads of the history: success when a received
         * file or note finished, failure when one stopped part way. Sends are left to the action
         * that started them.
         */
        fun arrival(previous: List<Transfer>, current: List<Transfer>): Feedback? {
            val before = previous.associate { it.id to it.status }
            var kind: Feedback? = null
            for (transfer in current) {
                if (transfer.direction != Direction.RECEIVED) continue
                val was = before[transfer.id]
                val wasRunning = was == null || was is TransferStatus.InProgress || was is TransferStatus.Waiting
                if (!wasRunning) continue
                when (transfer.status) {
                    is TransferStatus.Done -> if (kind == null) kind = Feedback.SUCCESS
                    is TransferStatus.Failed -> kind = Feedback.FAILURE
                    else -> Unit
                }
            }
            return kind
        }
    }
}
