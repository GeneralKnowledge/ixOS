package FORGE_JAVA_PACKAGE.ui

import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.launch
import FORGE_JAVA_PACKAGE.NotesViewModel

/**
 * The starter screen: a notes list kept by Rust, and a URL check that runs an HTTPS request from
 * Rust. Replace it with your own UI.
 */
@Composable
fun NotesScreen(context: Context) {
    val model: NotesViewModel = viewModel(factory = NotesViewModelFactory(context))
    val scope = rememberCoroutineScope()
    // Applies Rust's events while the screen is shown; cancelled when it leaves.
    LaunchedEffect(model) { model.listen() }

    MaterialTheme {
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("FORGE_APP_NAME", style = MaterialTheme.typography.headlineMedium)
            val openError = model.openError
            if (openError != null) {
                Text("Could not open your notes", style = MaterialTheme.typography.titleMedium)
                Text(openError)
                Button(onClick = { model.open() }) { Text("Retry") }
                return@Column
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = model.draft,
                    onValueChange = { model.draft = it },
                    label = { Text("New note") },
                    modifier = Modifier.weight(1f),
                    singleLine = true,
                )
                TextButton(
                    onClick = { scope.launch { model.addNote() } },
                    enabled = model.draft.isNotBlank(),
                ) { Text("Add") }
            }
            model.errorMessage?.let { Text(it, color = Color.Red) }

            LazyColumn(modifier = Modifier.weight(1f).fillMaxWidth()) {
                if (model.notes.isEmpty()) {
                    item { Text("No notes yet.", color = Color.Gray) }
                }
                items(model.notes, key = { it.id.toLong() }) { note ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(note.text, modifier = Modifier.weight(1f))
                        TextButton(onClick = { scope.launch { model.delete(note) } }) {
                            Text("Delete")
                        }
                    }
                }
            }

            Text("Network check", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(
                value = model.url,
                onValueChange = { model.url = it },
                label = { Text("URL") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = { scope.launch { model.checkUrl() } },
                enabled = !model.isCheckingUrl,
            ) { Text(if (model.isCheckingUrl) "Checking…" else "Check") }
            model.urlResult?.let { Text(it) }
        }
    }
}
