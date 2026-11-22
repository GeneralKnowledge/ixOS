// Sending and receiving with a word code (the iPhone app's SendCodeView.swift and
// ReceiveCodeView.swift): anyone with Relay, or the `relay` command, on the same Wi-Fi.
package org.forgemobile.relay.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import org.forgemobile.relay.CodeReceive
import org.forgemobile.relay.PickedFiles
import org.forgemobile.relay.RelayFormat
import org.forgemobile.relay.RelayModel
import org.forgemobile.relay.RelayNetwork
import org.forgemobile.relay.ui.theme.Forge
import org.forgemobile.relay.ui.theme.ForgeTextRole
import org.forgemobile.relay.ui.theme.toColor
import uniffi.relay_core.CodeSend

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendCodeScreen(
    model: RelayModel,
    network: RelayNetwork,
    onScreen: StateFlow<Boolean>,
    picked: PickedFiles,
    close: () -> Unit,
) {
    val send by model.codeSend.collectAsState()
    val visible by onScreen.collectAsState()
    val sending = RelayModel.isRunning(send)
    var choosing by remember { mutableStateOf(false) }

    fun start(uris: List<Uri>, name: String) {
        if (uris.isEmpty()) return
        // One code send at a time, so its copies can replace the last one's.
        model.sendPicked(name) {
            picked.release()
            try {
                picked.copy(uris)
            } catch (e: Exception) {
                picked.release()
                throw e
            }
        }
    }

    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        start(uris, if (uris.size == 1) "your file" else "${uris.size} files")
    }
    val photos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia()) { uris ->
        start(uris, if (uris.size == 1) "a photo" else "${uris.size} photos")
    }
    // A folder goes as one item, as on the iPhone: its tree is copied in first, file by file.
    val folder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        if (tree == null) return@rememberLauncherForActivityResult
        model.sendPicked("your folder") {
            picked.release()
            try {
                listOf(picked.copyTree(tree))
            } catch (e: Exception) {
                picked.release()
                throw e
            }
        }
    }

    // While there is a code: its status twice a second, and its advert on the network.
    LaunchedEffect(sending) {
        while (sending) {
            delay(500)
            model.refreshCodeSend()
            network.advertiseCode(model.codeAdvert)
        }
        network.advertiseCode(model.codeAdvert)
    }
    // Back on screen after passing the code on in another app: advertise it again.
    LaunchedEffect(visible) {
        if (visible) network.readvertiseCode(model.codeAdvert)
    }
    // Other senders nearby, so Rust picks a number nobody else is using.
    DisposableEffect(model) {
        model.resetCodeSend()
        network.watchCodeSenders(model::codeSendersNearby)
        onDispose {
            network.stopWatchingCodeSenders()
            if (!model.isSendingCode) {
                network.advertiseCode(null)
                picked.release()
            }
        }
    }
    // A code on its way isn't dropped by going back by accident: Stop ends it.
    BackHandler(enabled = sending) {}

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Send") },
                navigationIcon = {
                    TextButton(
                        onClick = { if (sending) model.cancelCodeSend() else close() },
                        modifier = Modifier.testTag("sendCode.close"),
                    ) { Text(if (sending) "Stop" else "Done") }
                },
                actions = {
                    Box {
                        TextButton(
                            onClick = { choosing = true },
                            enabled = !sending,
                            modifier = Modifier.testTag("sendCode.choose"),
                        ) { Text("Choose") }
                        DropdownMenu(expanded = choosing, onDismissRequest = { choosing = false }) {
                            DropdownMenuItem(
                                text = { Text("Files") },
                                onClick = {
                                    choosing = false
                                    files.launch(arrayOf("*/*"))
                                },
                                modifier = Modifier.testTag("sendCode.files"),
                            )
                            DropdownMenuItem(
                                text = { Text("Photos") },
                                onClick = {
                                    choosing = false
                                    photos.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo))
                                },
                                modifier = Modifier.testTag("sendCode.photos"),
                            )
                            DropdownMenuItem(
                                text = { Text("A Folder") },
                                onClick = {
                                    choosing = false
                                    folder.launch(null)
                                },
                                modifier = Modifier.testTag("sendCode.folder"),
                            )
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
        ) {
            SendStatus(send)
            Text(
                "The receiver needs Relay, or the relay command on a computer, on the same Wi-Fi. Several items arrive together in one folder.",
                style = Forge.text(ForgeTextRole.NOTE),
            )
        }
    }
}

@Composable
private fun SendStatus(send: CodeSend) {
    val context = LocalContext.current
    val tag = Modifier.testTag("sendCode.status")
    when (send) {
        is CodeSend.Idle -> Text(
            "Choose files, photos or a folder, and Relay shows a code for the receiver.",
            style = Forge.text(ForgeTextRole.METADATA),
            modifier = tag,
        )
        is CodeSend.Preparing -> Working("Getting ${send.name} ready…", tag)
        is CodeSend.Waiting -> Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = tag) {
            send.note?.let { Text(it, color = Forge.colors.warning.text.toColor()) }
            SelectionContainer {
                Text(
                    send.code,
                    style = MaterialTheme.typography.headlineSmall,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier
                        .testTag("sendCode.code")
                        .semantics { contentDescription = "Code ${send.code.replace('-', ' ')}" },
                )
            }
            Text(
                "${send.name}, ${RelayFormat.size(send.size)}. Waiting for the receiver to type the code… Keep Relay open: others can only find this code while it's on screen.",
                style = Forge.text(ForgeTextRole.NOTE),
            )
            OutlinedButton(
                onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Relay code", send.code))
                },
                modifier = Modifier.testTag("sendCode.copy"),
            ) { Text("Copy Code") }
        }
        is CodeSend.Sending -> Working("Sending ${send.name}, ${RelayFormat.size(send.size)}", tag)
        is CodeSend.Sent -> Column(modifier = tag.semantics(mergeDescendants = true) {}) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = Forge.colors.success.text.toColor())
                Text("  Sent ${send.name}", color = Forge.colors.success.text.toColor())
            }
            Text(
                "${RelayFormat.size(send.size)} in " +
                    String.format(Locale.ROOT, "%.1f s, %.1f MB/s", send.seconds, send.megabytesPerSecond),
                style = Forge.text(ForgeTextRole.NOTE),
            )
        }
        is CodeSend.Failed -> Problem(send.reason, tag)
    }
}

@Composable
private fun Working(text: String, modifier: Modifier) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = modifier.semantics(mergeDescendants = true) {}) {
        CircularProgressIndicator(Modifier.size(20.dp))
        Text("  $text")
    }
}

@Composable
private fun Problem(text: String, modifier: Modifier) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = modifier.semantics(mergeDescendants = true) {}) {
        Icon(Icons.Filled.Warning, contentDescription = null, tint = MaterialTheme.colorScheme.error)
        Text("  $text", color = MaterialTheme.colorScheme.error)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReceiveCodeScreen(model: RelayModel, network: RelayNetwork, close: () -> Unit) {
    val receive by model.codeReceive.collectAsState()
    var code by rememberSaveable { mutableStateOf("") }
    val busy = receive is CodeReceive.Finding || receive is CodeReceive.Receiving

    fun start() {
        val typed = code.trim()
        if (typed.isEmpty() || busy) return
        model.startReceive(typed) { number -> network.findCodeSender(number) }
    }

    DisposableEffect(model) {
        model.resetCodeReceive()
        onDispose { model.resetCodeReceive() }
    }
    // The size of what is on its way.
    LaunchedEffect(busy) {
        while (busy) {
            delay(250)
            model.refreshCodeReceive()
        }
    }
    BackHandler(enabled = busy) {}

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Receive") },
                navigationIcon = {
                    TextButton(
                        onClick = { if (busy) model.cancelCodeReceive() else close() },
                        modifier = Modifier.testTag("receiveCode.close"),
                    ) { Text(if (busy) "Stop" else "Done") }
                },
                actions = {
                    TextButton(
                        onClick = ::start,
                        enabled = code.isNotBlank() && !busy,
                        modifier = Modifier.testTag("receiveCode.start"),
                    ) { Text("Receive") }
                },
            )
        },
    ) { padding ->
        Column(
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
        ) {
            OutlinedTextField(
                value = code,
                onValueChange = { code = it },
                label = { Text("Code") },
                placeholder = { Text("7-acid-tidy-rug") },
                singleLine = true,
                enabled = !busy,
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.None,
                    autoCorrectEnabled = false,
                    keyboardType = KeyboardType.Ascii,
                    imeAction = ImeAction.Go,
                ),
                keyboardActions = KeyboardActions(onGo = { start() }),
                modifier = Modifier.fillMaxWidth().testTag("receiveCode.code"),
            )
            Text(
                "The number and three words the sender shows. Both devices need to be on the same Wi-Fi.",
                style = Forge.text(ForgeTextRole.NOTE),
            )
            ReceiveStatus(receive, model)
        }
    }
}

@Composable
private fun ReceiveStatus(receive: CodeReceive, model: RelayModel) {
    val context = LocalContext.current
    val tag = Modifier.testTag("receiveCode.status")
    when (receive) {
        CodeReceive.Idle -> Text("Type the code, then Receive.", style = Forge.text(ForgeTextRole.METADATA), modifier = tag)
        CodeReceive.Finding -> Working("Looking for the sender…", tag)
        is CodeReceive.Receiving -> Column(verticalArrangement = Arrangement.spacedBy(6.dp), modifier = tag) {
            val size = receive.size
            if (size == null) {
                Working("Checking the code…", Modifier)
            } else {
                Text("Receiving ${RelayFormat.size(size.total)}")
                LinearProgressIndicator(
                    progress = { if (size.total > 0u) (size.done.toDouble() / size.total.toDouble()).toFloat() else 0f },
                    modifier = Modifier.fillMaxWidth(),
                )
                Text("${RelayFormat.size(size.done)} of ${RelayFormat.size(size.total)}", style = Forge.text(ForgeTextRole.NOTE))
                if (size.already > 0u) {
                    Text("Resuming: ${RelayFormat.size(size.already)} already here.", style = Forge.text(ForgeTextRole.NOTE))
                }
            }
        }
        is CodeReceive.Received -> Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = tag.semantics(mergeDescendants = true) {}) {
                Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = Forge.colors.success.text.toColor())
                Text("  " + RelayFormat.receipt(receive.receipt))
            }
            Button(
                onClick = { ReceivedFiles.open(context, receive.receipt.path, model::report) },
                modifier = Modifier.testTag("receiveCode.open"),
            ) { Text("Open") }
        }
        is CodeReceive.Failed -> Problem(receive.message, tag)
    }
}
