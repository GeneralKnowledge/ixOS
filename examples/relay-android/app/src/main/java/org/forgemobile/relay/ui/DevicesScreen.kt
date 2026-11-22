// Screen 1: trusted devices and the history (the iPhone app's DevicesView in ContentView.swift).
// While it is on screen, Relay advertises its iroh endpoint for trusted devices and looks for
// them.
package org.forgemobile.relay.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.BottomAppBar
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.forgemobile.relay.RelayErrors
import org.forgemobile.relay.RelayFormat
import org.forgemobile.relay.RelayModel
import org.forgemobile.relay.RelayNetwork
import org.forgemobile.relay.ServerChoice
import org.forgemobile.relay.ui.theme.Forge
import org.forgemobile.relay.ui.theme.ForgeTextRole
import org.forgemobile.relay.ui.theme.toColor
import uniffi.relay_core.ConnectionMode
import uniffi.relay_core.Direction
import uniffi.relay_core.Item
import uniffi.relay_core.PairingPolicy
import uniffi.relay_core.Presence
import uniffi.relay_core.RelayException
import uniffi.relay_core.ServerHealth
import uniffi.relay_core.Transfer
import uniffi.relay_core.TransferStatus

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DevicesScreen(
    model: RelayModel,
    network: RelayNetwork,
    onScreen: StateFlow<Boolean>,
    open: (Route) -> Unit,
) {
    val state by model.state.collectAsState()
    val visible by onScreen.collectAsState()
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var renaming by remember { mutableStateOf(false) }

    // Progress of what is on its way, twice a second; what waits in the outbox, every two
    // seconds, to see it go.
    LaunchedEffect(state.isTransferring, state.isWaiting) {
        while (state.isTransferring || state.isWaiting) {
            delay(if (state.isTransferring) 500 else 2_000)
            model.reload()
        }
    }
    // Presence ages: "reachable" turns "offline" a few minutes after the last connection.
    LaunchedEffect(model) {
        while (true) {
            delay(30_000)
            model.refreshDevices()
        }
    }
    // The relay can go down or come back with nothing else changing.
    LaunchedEffect(model) {
        while (true) {
            delay(5_000)
            model.refreshServerHealth()
        }
    }
    // Open to trusted devices while on screen: advertise, and look for them. Android stops
    // neither in the background by itself, so leaving the screen withdraws both.
    DisposableEffect(model, visible) {
        val job = if (visible) {
            scope.launch {
                network.watchDevices(model::devicesNearby)
                network.advertiseDevice(model.deviceService())
            }
        } else {
            null
        }
        onDispose {
            job?.cancel()
            network.advertiseDevice(null)
            network.stopWatchingDevices()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Relay") },
                actions = {
                    IconButton(onClick = { renaming = true }, modifier = Modifier.testTag("devices.rename")) {
                        Icon(Icons.Filled.Edit, contentDescription = "Rename This Device")
                    }
                },
            )
        },
        bottomBar = {
            // Send and Receive work with anyone on this Wi-Fi, by word code; trusted devices are
            // reached from their own screen.
            BottomAppBar {
                TextButton(onClick = { open(Route.Receive) }, modifier = Modifier.testTag("relay.receive")) {
                    Icon(Icons.Filled.KeyboardArrowDown, contentDescription = null)
                    Text("Receive")
                }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = { open(Route.Send) }, modifier = Modifier.testTag("relay.send")) {
                    Icon(Icons.Filled.KeyboardArrowUp, contentDescription = null)
                    Text("Send")
                }
            }
        },
    ) { padding ->
        if (state.isEmpty) {
            EmptyDevices(Modifier.padding(padding)) { open(Route.Send) }
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(padding)) {
                // At the top, not only under Connections: a relay that can't be reached is why
                // a device away from home doesn't answer.
                if (state.connectionMode == ConnectionMode.HYBRID && state.serverHealth == ServerHealth.UNREACHABLE) {
                    item(key = "serversDown") { ServersDownNotice() }
                }
                if (state.trustedDevices.isNotEmpty()) {
                    sectionHeader("Trusted Devices", "trusted")
                    items(state.trustedDevices, key = { "trusted." + it.endpointId }) { device ->
                        TrustedDeviceRow(device) { open(Route.Device(device.endpointId)) }
                    }
                    sectionFooter(
                        "“${state.identity.name}” is open to trusted devices while Relay is on screen.",
                        "trusted",
                    )
                }
                if (state.revokedDevices.isNotEmpty()) {
                    sectionHeader("Revoked", "revoked")
                    items(state.revokedDevices, key = { "revoked." + it.endpointId }) { device ->
                        RevokedDeviceRow(
                            device,
                            trustAgain = { scope.launch { model.trustAgain(device) } },
                            forget = { model.askToForget(device) },
                        )
                    }
                }
                sectionHeader("Pairing", "pairing")
                item(key = "policy") { PolicyRow(state.policy) { scope.launch { model.setPolicy(it) } } }
                sectionFooter(RelayFormat.explain(state.policy), "pairing")
                connectionSection(
                    state.connectionMode,
                    state.servers,
                    state.serverHealth,
                    showsDown = false,
                    choose = { scope.launch { model.setConnectionMode(it) } },
                    openServers = { open(Route.Servers) },
                )
                if (state.transfers.isNotEmpty()) {
                    sectionHeader("History", "history")
                    items(state.transfers, key = { "history." + it.id }) { transfer ->
                        TransferRow(transfer, model)
                    }
                }
            }
        }
    }

    if (renaming) {
        RenameDialog(
            name = state.identity.name,
            onDismiss = { renaming = false },
            rename = { model.rename(it) },
        )
    }
    Notices(model)
}

@Composable
private fun EmptyDevices(modifier: Modifier, sendFirst: () -> Unit) {
    Box(modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("No Devices", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("devices.empty"))
            Text(
                "Send something with a code once, then trust the other device to send notes and files straight to it, encrypted.",
                style = MaterialTheme.typography.bodyMedium,
                textAlign = TextAlign.Center,
            )
            Button(onClick = sendFirst, modifier = Modifier.testTag("devices.sendFirst")) { Text("Send with a Code") }
        }
    }
}

/**
 * What Relay tells the person once the screen is clear: a trust offer after a code transfer,
 * saved data that couldn't be read, or Relay's servers turned on by an upgrade. One at a time,
 * and none while another alert is up.
 */
@Composable
private fun Notices(model: RelayModel) {
    val state by model.state.collectAsState()
    val alert by model.alert.collectAsState()
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    if (alert != null) return
    val saved = state.savedDataNotice
    val away = state.awayNotice
    val offer = state.trustOffer
    when {
        saved != null -> AlertDialog(
            onDismissRequest = {},
            title = { Text("Some Saved Data Couldn't Be Read") },
            text = { Text(saved.message) },
            confirmButton = {
                TextButton(
                    onClick = { scope.launch { model.dismissSavedDataNotice() } },
                    modifier = Modifier.testTag("savedData.ok"),
                ) { Text("OK") }
            },
        )
        away != null -> AlertDialog(
            onDismissRequest = {},
            title = { Text("Relay Works Away From Home") },
            text = { Text(away) },
            confirmButton = {
                TextButton(
                    onClick = { scope.launch { model.dismissAwayNotice() } },
                    modifier = Modifier.testTag("awayNotice.ok"),
                ) { Text("OK") }
            },
        )
        offer != null -> AlertDialog(
            // Trust and Not Now answer it; nothing else closes it.
            onDismissRequest = {},
            title = { Text("Trust “${offer.name}”?") },
            text = {
                Text(
                    offer.introducedBy?.let {
                        "$it trusts it. You'll be able to send to each other without a code. You can revoke this at any time."
                    } ?: "You'll be able to send to each other without a code. You can revoke this at any time.",
                )
            },
            confirmButton = {
                TextButton(
                    onClick = { scope.launch { model.trust(offer) } },
                    modifier = Modifier.testTag("trustOffer.trust"),
                ) { Text("Trust") }
            },
            dismissButton = {
                TextButton(onClick = model::dismissTrustOffer, modifier = Modifier.testTag("trustOffer.notNow")) {
                    Text("Not Now")
                }
            },
        )
    }
}

@Composable
private fun RenameDialog(name: String, onDismiss: () -> Unit, rename: suspend (String) -> Unit) {
    var text by remember { mutableStateOf(name) }
    var problem by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Rename This Device") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Other devices see this name.")
                OutlinedTextField(
                    value = text,
                    onValueChange = {
                        text = it
                        problem = null
                    },
                    singleLine = true,
                    isError = problem != null,
                    supportingText = problem?.let { { Text(it) } },
                    modifier = Modifier.testTag("rename.name"),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    scope.launch {
                        try {
                            rename(text)
                            onDismiss()
                        } catch (e: RelayException) {
                            problem = RelayErrors.describe(e)
                        }
                    }
                },
                modifier = Modifier.testTag("rename.save"),
            ) { Text("Save") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, modifier = Modifier.testTag("rename.cancel")) { Text("Cancel") }
        },
    )
}

@Composable
private fun TrustedDeviceRow(device: uniffi.relay_core.Device, onClick: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .testTag("devices.trusted")
            .clickable(onClick = onClick)
            .semantics(mergeDescendants = true) {}
            .padding(horizontal = 16.dp, vertical = 10.dp),
    ) {
        Icon(
            Icons.Filled.CheckCircle,
            contentDescription = null,
            tint = if (device.presence == Presence.OFFLINE) {
                MaterialTheme.colorScheme.onSurfaceVariant
            } else {
                MaterialTheme.colorScheme.primary
            },
        )
        Spacer(Modifier.width(12.dp))
        Column {
            Text(RelayFormat.rowName(device), style = MaterialTheme.typography.bodyLarge)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TrustBadge()
                Text(RelayFormat.whereNow(device), style = Forge.text(ForgeTextRole.NOTE))
            }
        }
    }
}

@Composable
fun TrustBadge() {
    Text("Trusted", style = MaterialTheme.typography.labelMedium, color = Forge.colors.success.text.toColor())
}

@Composable
private fun RevokedDeviceRow(device: uniffi.relay_core.Device, trustAgain: () -> Unit, forget: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
    ) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}) {
            Text(device.name, style = MaterialTheme.typography.bodyLarge)
            Text(RelayFormat.revoked(device), style = Forge.text(ForgeTextRole.NOTE))
        }
        TextButton(onClick = trustAgain, modifier = Modifier.testTag("revoked.trustAgain")) { Text("Trust Again") }
        TextButton(onClick = forget, modifier = Modifier.testTag("revoked.forget")) { Text("Forget") }
    }
}

@Composable
private fun PolicyRow(policy: PairingPolicy, choose: (PairingPolicy) -> Unit) {
    ChoiceRow(
        label = "After a Code Transfer",
        choice = policy,
        choices = listOf(PairingPolicy.ASK_TO_PAIR, PairingPolicy.PAIR_AUTOMATICALLY, PairingPolicy.CODES_ONLY),
        name = RelayFormat::policyName,
        tag = "devices.policy",
        onChoose = choose,
    )
}

/** Connections: which routes reach trusted devices, and (Everywhere) which servers help. */
fun androidx.compose.foundation.lazy.LazyListScope.connectionSection(
    mode: ConnectionMode,
    servers: uniffi.relay_core.RelayServers,
    health: ServerHealth,
    showsDown: Boolean,
    choose: (ConnectionMode) -> Unit,
    openServers: () -> Unit,
) {
    sectionHeader("Connections", "connections")
    item(key = "connectionMode") {
        ChoiceRow(
            label = "Reach Your Devices",
            choice = mode,
            choices = listOf(ConnectionMode.HYBRID, ConnectionMode.PRIVATE, ConnectionMode.LOCAL),
            name = RelayFormat::modeName,
            tag = "devices.connectionMode",
            onChoose = choose,
        )
    }
    if (mode == ConnectionMode.HYBRID) {
        item(key = "servers") {
            ValueRow("Servers", ServerChoice.of(servers).shortName, "devices.servers", onClick = openServers)
        }
        item(key = "serverHealth") { ServerHealthRow(health, showsDown) }
    }
    sectionFooter(RelayFormat.explain(mode, servers), "connections")
}

@Composable
fun ServerHealthRow(health: ServerHealth, showsDown: Boolean = true) {
    when {
        health == ServerHealth.CONNECTING -> Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .padding(horizontal = 16.dp, vertical = 8.dp)
                .testTag("devices.serversChecking")
                .semantics(mergeDescendants = true) {},
        ) {
            CircularProgressIndicator(Modifier.padding(end = 8.dp).width(16.dp))
            Text("Checking the servers…", style = Forge.text(ForgeTextRole.NOTE))
        }
        health == ServerHealth.UNREACHABLE && showsDown -> ServersDownNotice()
        else -> Unit
    }
}

@Composable
fun ServersDownNotice() {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .testTag("devices.serversDown")
            .semantics(mergeDescendants = true) {}
            .padding(16.dp),
    ) {
        Icon(Icons.Filled.Warning, contentDescription = null, tint = Forge.colors.warning.text.toColor())
        Spacer(Modifier.width(12.dp))
        Text(
            "The servers can't be reached right now. Devices on the same Wi-Fi or Tailscale still can.",
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

/** One note or file in the history. A received file opens; a note can be copied. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TransferRow(transfer: Transfer, model: RelayModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var menu by remember { mutableStateOf(false) }
    val item = transfer.item
    val pending = transfer.status is TransferStatus.Waiting || transfer.status is TransferStatus.InProgress
    Box {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .testTag("history.row")
                .combinedClickable(
                    onClick = {
                        if (item is Item.File) {
                            item.path?.let { ReceivedFiles.open(context, it, model::report) }
                        }
                    },
                    onLongClick = { menu = true },
                    onLongClickLabel = "More",
                )
                .semantics(mergeDescendants = true) {}
                .padding(horizontal = 16.dp, vertical = 10.dp),
        ) {
            Icon(
                if (transfer.direction == Direction.SENT) Icons.Filled.KeyboardArrowUp else Icons.Filled.KeyboardArrowDown,
                contentDescription = if (transfer.direction == Direction.SENT) "Sent" else "Received",
                tint = if (transfer.direction == Direction.SENT) {
                    MaterialTheme.colorScheme.primary
                } else {
                    Forge.colors.success.text.toColor()
                },
            )
            Spacer(Modifier.width(12.dp))
            Column {
                Text(RelayFormat.title(item), maxLines = 3, overflow = TextOverflow.Ellipsis)
                val details = buildList {
                    add(RelayFormat.peer(transfer))
                    if (item is Item.File) add(RelayFormat.size(item.size))
                    add(RelayFormat.time(transfer.atMs))
                }
                Text(details.joinToString("  "), style = Forge.text(ForgeTextRole.NOTE))
                if (transfer.status !is TransferStatus.Done) {
                    Text(
                        RelayFormat.status(transfer),
                        style = Forge.text(ForgeTextRole.NOTE),
                        color = if (transfer.status is TransferStatus.Failed) {
                            MaterialTheme.colorScheme.error
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    )
                }
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            if (item is Item.Note) {
                DropdownMenuItem(
                    text = { Text("Copy") },
                    onClick = {
                        menu = false
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        clipboard.setPrimaryClip(ClipData.newPlainText("Relay note", item.text))
                    },
                    modifier = Modifier.testTag("history.copy"),
                )
            }
            if (transfer.direction == Direction.SENT && pending) {
                DropdownMenuItem(
                    text = { Text("Cancel Sending") },
                    onClick = {
                        menu = false
                        model.cancel(transfer)
                    },
                    modifier = Modifier.testTag("history.cancel"),
                )
            }
            DropdownMenuItem(
                text = { Text("Remove from History") },
                onClick = {
                    menu = false
                    scope.launch { model.remove(transfer) }
                },
                modifier = Modifier.testTag("history.remove"),
            )
        }
    }
}
