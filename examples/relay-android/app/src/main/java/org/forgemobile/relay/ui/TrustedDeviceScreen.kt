// A trusted device's page (the iPhone app's TrustedDeviceView in TrustedDeviceViews.swift): send
// it files, see where it is and what it says about itself, and what it may do here. And the
// Servers screen that Connections opens.
package org.forgemobile.relay.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.forgemobile.relay.PickedFiles
import org.forgemobile.relay.RelayFormat
import org.forgemobile.relay.RelayModel
import org.forgemobile.relay.ServerChoice
import uniffi.relay_core.Capability
import uniffi.relay_core.Device
import uniffi.relay_core.RelayServers
import uniffi.relay_core.ServerHealth
import uniffi.relay_core.ownRelayServers

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TrustedDeviceScreen(
    model: RelayModel,
    endpointId: String,
    picked: PickedFiles,
    back: () -> Unit,
    /** Opens this device's conversation in Chats. */
    message: () -> Unit,
) {
    val state by model.state.collectAsState()
    val device = state.device(endpointId)?.takeIf { it.trusted }
    var menu by remember { mutableStateOf(false) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(device?.name ?: "No Longer Trusted") },
                navigationIcon = {
                    IconButton(onClick = back, modifier = Modifier.testTag("trusted.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (device != null) {
                        Box {
                            IconButton(onClick = { menu = true }, modifier = Modifier.testTag("trusted.more")) {
                                Icon(Icons.Filled.MoreVert, contentDescription = "More")
                            }
                            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                DropdownMenuItem(
                                    text = { Text("Revoke Trust") },
                                    onClick = {
                                        menu = false
                                        model.askToRevoke(device)
                                    },
                                    modifier = Modifier.testTag("trusted.revoke"),
                                )
                                DropdownMenuItem(
                                    text = { Text("Forget Device") },
                                    onClick = {
                                        menu = false
                                        model.askToForget(device)
                                    },
                                    modifier = Modifier.testTag("trusted.forget"),
                                )
                            }
                        }
                    }
                },
            )
        },
    ) { padding ->
        if (device == null) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                Text("No Longer Trusted", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("trusted.gone"))
            }
        } else {
            DeviceDetails(model, device, picked, message, Modifier.padding(padding))
        }
    }
}

@Composable
private fun DeviceDetails(model: RelayModel, device: Device, picked: PickedFiles, message: () -> Unit, modifier: Modifier) {
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var sending by remember { mutableStateOf(false) }
    var asking by remember { mutableStateOf(false) }
    var namingPerson by rememberSaveable { mutableStateOf(false) }
    var personName by rememberSaveable(device.endpointId) { mutableStateOf(device.person ?: "") }
    var syncing by remember { mutableStateOf(false) }
    var synced by remember { mutableStateOf<String?>(null) }

    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        sending = true
        // Relay copies them in, so the picked copies go once the send returns: only this
        // send's, as another device's send may be copying in beside it.
        model.sendPicked(device, copy = { picked.copy(uris) }) { paths ->
            picked.release(paths)
            sending = false
        }
    }

    // Asks it about itself each time the page opens: offers can be hours old.
    LaunchedEffect(device.endpointId) {
        asking = true
        model.refreshStatus(device)
        asking = false
    }

    LazyColumn(modifier.fillMaxSize()) {
        item(key = "badge") {
            Row(Modifier.padding(start = 16.dp, top = 16.dp)) { TrustBadge() }
        }
        item(key = "send") {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                Button(onClick = message, modifier = Modifier.testTag("trusted.message")) {
                    Text("Message")
                }
                Spacer(Modifier.width(8.dp))
                OutlinedButton(
                    onClick = { files.launch(arrayOf("*/*")) },
                    enabled = !sending,
                    modifier = Modifier.testTag("trusted.sendFiles"),
                ) {
                    if (sending) CircularProgressIndicator(Modifier.size(16.dp))
                    Text(if (sending) "  Sending…" else "Send Files")
                }
            }
        }
        sectionFooter(RelayFormat.reach(device), "reach")

        sectionHeader("About This Device", "about")
        item(key = "where") { ValueRow("Where", RelayFormat.whereNow(device), "trusted.where") }
        item(key = "platform") {
            ValueRow("Platform", device.platform?.let(RelayFormat::platformName) ?: "Not known yet", "trusted.platform")
        }
        item(key = "offers") {
            ValueRow("Offers", device.offers.ifEmpty { null }?.joinToString(", ") ?: "Not known yet", "trusted.offers")
        }
        item(key = "ask") {
            TextButton(
                onClick = {
                    scope.launch {
                        asking = true
                        model.refreshStatus(device)
                        asking = false
                    }
                },
                enabled = !asking,
                modifier = Modifier.padding(horizontal = 8.dp).testTag("trusted.askStatus"),
            ) {
                Icon(Icons.Filled.Refresh, contentDescription = null)
                Text(if (asking) " Asking…" else " Ask Again")
            }
        }
        sectionFooter("What ${device.name} says about itself. It grants nothing here.", "about")

        item(key = "receives") {
            SwitchRow("Can Send to This Device", device.receivesFrom, "trusted.receivesFrom") { on ->
                scope.launch { model.setReceives(on, device) }
            }
        }
        sectionFooter(
            if (device.receivesFrom) {
                "${device.name} can send messages and files here without a code."
            } else {
                "${device.name} is still trusted, but what it sends here is declined."
            },
            "receives",
        )

        sectionHeader("Sites", "sites")
        item(key = "opensSite") {
            SwitchRow("Can Open My Site", Capability.SITES in device.grants, "trusted.opensSite") { on ->
                scope.launch { model.setGrant(Capability.SITES, on, device) }
            }
        }
        sectionFooter(
            if ("sites" in device.offers) {
                "${device.name} shares a site with this device. Opening it in Relay for Android comes in a later version."
            } else {
                "When on, ${device.name} can open a site this device shares. Trust alone never lets it in."
            },
            "sites",
        )

        sectionHeader("Whose Device", "person")
        item(key = "isPerson") {
            SwitchRow("This Is a Person", device.person != null || namingPerson, "trusted.isPerson") { on ->
                if (on) {
                    personName = device.person ?: ""
                    namingPerson = true
                } else {
                    namingPerson = false
                    scope.launch { model.setPerson(null, device) }
                }
            }
        }
        if (device.person != null || namingPerson) {
            item(key = "personName") {
                OutlinedTextField(
                    value = personName,
                    onValueChange = { personName = it },
                    label = { Text("Their Name") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.Words,
                        imeAction = ImeAction.Done,
                    ),
                    keyboardActions = KeyboardActions(onDone = {
                        val name = personName.trim()
                        if (name.isNotEmpty()) {
                            scope.launch {
                                model.setPerson(name, device)
                                namingPerson = false
                            }
                        }
                    }),
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp)
                        .testTag("trusted.personName"),
                )
            }
        }
        sectionFooter(
            device.person?.let {
                "${device.name} belongs to $it. It can send messages and files, but never introduces devices or sees yours."
            } ?: if (namingPerson) {
                "Type their name and press Done."
            } else {
                "One of your own devices. Turn on if it belongs to someone you trust."
            },
            "person",
        )

        if (device.person == null) {
            sectionHeader("Your Devices", "yours")
            item(key = "introduces") {
                SwitchRow("Introduces Devices", device.introduces, "trusted.introduces") { on ->
                    scope.launch { model.setIntroduces(on, device) }
                }
            }
            item(key = "seesDevices") {
                SwitchRow("Can See My Devices", device.seesDevices, "trusted.seesDevices") { on ->
                    scope.launch { model.setSeesDevices(on, device) }
                }
            }
            if (device.introduces) {
                item(key = "sync") {
                    TextButton(
                        onClick = {
                            scope.launch {
                                syncing = true
                                synced = model.syncDevices(device)
                                syncing = false
                            }
                        },
                        enabled = !syncing,
                        modifier = Modifier.padding(horizontal = 8.dp).testTag("trusted.syncDevices"),
                    ) { Text(if (syncing) "Asking…" else "Check Its Devices Now") }
                }
            }
            device.introducedBy?.let { introducer ->
                item(key = "introducedBy") { ValueRow("Introduced By", introducer, "trusted.introducedBy") }
            }
            sectionFooter(
                synced ?: if (device.introduces) {
                    "Devices ${device.name} trusts are offered here for you to trust, and devices it revokes are revoked here too."
                } else {
                    "Turn on for your own devices, so you pair and revoke once. Both devices need it on: Introduces here, Can See My Devices there."
                },
                "yours",
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServersScreen(model: RelayModel, back: () -> Unit) {
    val state by model.state.collectAsState()
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    val initial = ServerChoice.of(state.servers)
    var choice by rememberSaveable { mutableStateOf(initial) }
    var relayUrl by rememberSaveable { mutableStateOf(if (initial == ServerChoice.OWN) state.servers.relayUrl ?: "" else "") }
    var lookupUrl by rememberSaveable { mutableStateOf(if (initial == ServerChoice.OWN) state.servers.lookupUrl ?: "" else "") }
    val own = ServerChoice.own(relayUrl, lookupUrl)

    LaunchedEffect(model) {
        while (true) {
            model.refreshServerHealth()
            delay(1_000)
        }
    }

    fun choose(chosen: ServerChoice) {
        choice = chosen
        when (chosen) {
            ServerChoice.RELAYS -> scope.launch { model.setServers(ownRelayServers()) }
            ServerChoice.NONE -> scope.launch { model.setServers(RelayServers(null, null)) }
            ServerChoice.OWN -> Unit
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Servers") },
                navigationIcon = {
                    IconButton(onClick = back, modifier = Modifier.testTag("servers.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.fillMaxSize().padding(padding)) {
            ServerChoice.entries.forEach { option ->
                item(key = "choice.${option.name}") {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier
                            .fillMaxWidth()
                            .testTag("servers.choice.${option.name.lowercase()}")
                            .selectable(selected = choice == option, role = Role.RadioButton) { choose(option) }
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                    ) {
                        RadioButton(selected = choice == option, onClick = null)
                        Text("  ${option.title}", style = MaterialTheme.typography.bodyLarge)
                    }
                }
            }
            sectionFooter(choice.explanation, "choice")
            if (state.serverHealth == ServerHealth.CONNECTING || state.serverHealth == ServerHealth.UNREACHABLE) {
                item(key = "health") { ServerHealthRow(state.serverHealth) }
            }
            if (choice == ServerChoice.OWN) {
                sectionHeader("Your Own Servers", "own")
                item(key = "relayUrl") {
                    UrlField(relayUrl, "Relay, https://…", "servers.relayURL") { relayUrl = it }
                }
                item(key = "lookupUrl") {
                    UrlField(lookupUrl, "Lookup, https://…/pkarr", "servers.lookupURL") { lookupUrl = it }
                }
                item(key = "useOwn") {
                    OutlinedButton(
                        onClick = { scope.launch { model.setServers(own) } },
                        enabled = own.relayUrl != null || own.lookupUrl != null,
                        modifier = Modifier.padding(16.dp).testTag("servers.useOwn"),
                    ) { Text("Use These Servers") }
                }
            }
        }
    }
}

@Composable
private fun UrlField(value: String, label: String, tag: String, onChange: (String) -> Unit) {
    Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
        OutlinedTextField(
            value = value,
            onValueChange = onChange,
            label = { Text(label) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.None,
                autoCorrectEnabled = false,
                keyboardType = KeyboardType.Uri,
            ),
            modifier = Modifier.fillMaxWidth().testTag(tag),
        )
    }
}
