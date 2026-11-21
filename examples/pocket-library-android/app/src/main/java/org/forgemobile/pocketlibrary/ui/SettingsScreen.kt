// Settings. For now only online search (the catalogue); the rest of the iPhone app's Settings
// (default sort, backups, storage) comes with plan milestone A6.
package org.forgemobile.pocketlibrary.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.ui.theme.Forge

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SettingsScreen(model: LibraryModel, onClose: () -> Unit) {
    val settings by model.settings.collectAsState()
    var alert by remember { mutableStateOf<ForgeAlert<Nothing>?>(null) }
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(onClick = onClose, modifier = Modifier.testTag("settings.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Library")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).padding(Forge.spacing.l.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .toggleable(value = settings.catalogueEnabled, role = Role.Switch) { enabled ->
                        scope.launch {
                            try {
                                model.setCatalogueEnabled(enabled)
                            } catch (e: Exception) {
                                alert = ForgeAlert.failure(e)
                            }
                        }
                    }
                    .testTag("settings.onlineCatalogue")
                    .padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Online Catalogue", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                Switch(checked = settings.catalogueEnabled, onCheckedChange = null)
            }
            Text(
                "Search Open Library to add books with their covers and descriptions. When this is off, Pocket Library makes no catalogue requests.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    ForgeAlertDialog(alert, onDismiss = { alert = null }) {}
}
