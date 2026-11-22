// The chat server, from the Chats tab (the iPhone's SettingsView and SignUpView): join one with
// a name, see the account, unmute conversations, sign out. Trusted devices are conversations
// without any of this; a chat server adds rooms with other people. And New Conversation, which
// makes a room on it.
package org.forgemobile.relay.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.forgemobile.relay.ChatModel
import org.forgemobile.relay.kit.ForgeFailure

/** The servers the form suggests: one on the computer next to you (`make chat-server`). */
object ChatServers {
    const val LOCAL = "http://192.168.1.10:8787"
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatServerScreen(model: ChatModel, back: () -> Unit) {
    val state by model.state.collectAsState()
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var name by rememberSaveable { mutableStateOf("") }
    var server by rememberSaveable { mutableStateOf("") }
    var working by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<ForgeFailure?>(null) }
    var confirmingSignOut by remember { mutableStateOf(false) }
    val account = state.account

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Chat Server") },
                navigationIcon = {
                    IconButton(onClick = back, modifier = Modifier.testTag("chatServer.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.fillMaxSize().padding(padding)) {
            if (account != null) {
                sectionHeader("Account", "account")
                item(key = "name") { ValueRow("Name", account.displayName, "chatServer.name") }
                item(key = "server") { ValueRow("Server", account.serverUrl, "chatServer.server") }
                item(key = "id") { ValueRow("Account ID", account.userId, "chatServer.accountId") }
                state.accountProblem?.let { problem ->
                    item(key = "problem") {
                        Notice(
                            "The server no longer accepts this account ($problem). Sign out and create a new one.",
                            "chatServer.problem",
                        )
                    }
                }
            } else {
                sectionHeader("Join a Chat Server", "join")
                item(key = "form") {
                    val canSubmit = !working && name.isNotBlank() && server.isNotBlank()
                    val submit = {
                        if (canSubmit) {
                            working = true
                            scope.launch {
                                try {
                                    model.signUp(server, name)
                                } catch (e: Exception) {
                                    failure = ForgeFailure.from(e).copy(title = "Can't Create the Account")
                                } finally {
                                    working = false
                                }
                            }
                        }
                    }
                    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = name,
                            onValueChange = { name = it },
                            label = { Text("Your name") },
                            supportingText = { Text("Shown next to your messages.") },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words, imeAction = ImeAction.Next),
                            modifier = Modifier.fillMaxWidth().testTag("signUp.name"),
                        )
                        OutlinedTextField(
                            value = server,
                            onValueChange = { server = it },
                            label = { Text("Chat server") },
                            placeholder = { Text(ChatServers.LOCAL) },
                            supportingText = {
                                Text("Run `make chat-server ARGS=\"--bind 0.0.0.0:8787\"` on your computer and enter its address on your network.")
                            },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                            keyboardActions = androidx.compose.foundation.text.KeyboardActions(onGo = { submit() }),
                            modifier = Modifier.fillMaxWidth().testTag("signUp.server"),
                        )
                        Button(onClick = { submit() }, enabled = canSubmit, modifier = Modifier.testTag("signUp.create")) {
                            if (working) CircularProgressIndicator(Modifier.size(16.dp))
                            Text(if (working) "  Creating…" else "Create Account")
                        }
                    }
                }
                sectionFooter(
                    "Your trusted devices are in Chats already. A chat server adds conversations with other people. " +
                        "Your account is kept in this phone's secure store.",
                    "join",
                )
            }

            sectionHeader("Muted Conversations", "muted")
            if (state.mutedConversations.isEmpty()) {
                item(key = "none") { SectionText("None") }
            }
            items(state.mutedConversations, key = { "muted-${it.id}" }) { conversation ->
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                ) {
                    Text(conversation.title, modifier = Modifier.weight(1f))
                    TextButton(
                        onClick = { model.setMuted(conversation.id, false) },
                        modifier = Modifier.testTag("chatServer.unmute"),
                    ) { Text("Unmute") }
                }
            }
            sectionFooter(
                "Muted conversations still show unread messages, without notifications on this phone. " +
                    "Notifications arrive while Relay is open or was used recently: there is no push service, so they can be late.",
                "muted",
            )

            if (account != null) {
                item(key = "signOut") {
                    OutlinedButton(
                        onClick = { confirmingSignOut = true },
                        modifier = Modifier.padding(16.dp).testTag("chatServer.signOut"),
                    ) { Text("Sign Out", color = MaterialTheme.colorScheme.error) }
                }
                sectionFooter(
                    "Removes the account and its chats from this phone. The chat server has no sign-in, so you can't use this " +
                        "account again: you'd sign up as someone new. Your sent messages stay on the server.",
                    "signOut",
                )
            }
        }
    }

    failure?.let { shown ->
        AlertDialog(
            onDismissRequest = { failure = null },
            title = { Text(shown.title) },
            text = { Text(shown.message) },
            confirmButton = {
                TextButton(onClick = { failure = null }, modifier = Modifier.testTag("alert.ok")) { Text("OK") }
            },
        )
    }
    if (confirmingSignOut) {
        AlertDialog(
            onDismissRequest = { confirmingSignOut = false },
            title = { Text("Sign Out of the Chat Server?") },
            text = { Text("You can't sign back in to this account later.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmingSignOut = false
                        model.signOut()
                    },
                    modifier = Modifier.testTag("chatServer.confirmSignOut.confirm"),
                ) { Text("Sign Out", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirmingSignOut = false }, modifier = Modifier.testTag("chatServer.confirmSignOut.cancel")) {
                    Text("Cancel")
                }
            },
        )
    }
}

/** Starts a room on the chat server; stays open, with the title, if that fails. */
@Composable
fun NewConversationDialog(model: ChatModel, dismiss: () -> Unit, created: (String) -> Unit) {
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    var title by rememberSaveable { mutableStateOf("") }
    var working by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    val create = {
        if (title.isNotBlank() && !working) {
            working = true
            scope.launch {
                try {
                    created(model.createConversation(title))
                } catch (e: Exception) {
                    problem = ChatModel.describe(e)
                } finally {
                    working = false
                }
            }
        }
    }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("New Conversation") },
        text = {
            Column {
                OutlinedTextField(
                    value = title,
                    onValueChange = {
                        title = it
                        problem = null
                    },
                    label = { Text("Title") },
                    singleLine = true,
                    isError = problem != null,
                    supportingText = { Text(problem ?: "Everyone with an account on this server can see and join it.") },
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
                    keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { create() }),
                    modifier = Modifier.testTag("newConversation.title"),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { create() }, enabled = title.isNotBlank() && !working, modifier = Modifier.testTag("newConversation.confirm")) {
                Text(if (working) "Creating…" else "Create")
            }
        },
        dismissButton = {
            TextButton(onClick = dismiss, modifier = Modifier.testTag("newConversation.cancel")) { Text("Cancel") }
        },
    )
}
