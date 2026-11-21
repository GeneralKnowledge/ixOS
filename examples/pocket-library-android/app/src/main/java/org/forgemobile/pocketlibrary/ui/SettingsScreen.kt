// Settings, as the iPhone app's: preferences (the library's order, online search), the person's
// data (what the library holds, backups, spreadsheets, erasing) and About. Where the iPhone app
// shares a file, Android saves it where the person chooses (the system's Save dialog), and
// restores and imports through the system's document picker. Rust writes and reads the files;
// this screen only copies them to and from the document the person picked.
package org.forgemobile.pocketlibrary.ui

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.text.format.Formatter
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import java.io.File
import java.io.IOException
import java.util.UUID
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertButton
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.ui.theme.Forge
import uniffi.pocket_library_core.CsvImportSummary
import uniffi.pocket_library_core.LibrarySort
import uniffi.pocket_library_core.StorageInfo

/** What a confirmation in Settings goes on to do. */
private sealed interface SettingsAction {
    data object Erase : SettingsAction
    data class Restore(val uri: Uri) : SettingsAction
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SettingsScreen(model: LibraryModel, onClose: () -> Unit) {
    val context = LocalContext.current
    val settings by model.settings.collectAsState()
    val all by model.all.collectAsState()
    var alert by remember { mutableStateOf<ForgeAlert<SettingsAction>?>(null) }
    // Backups and the rest run in the model, so they carry on, and still show, if Settings
    // closes and opens again.
    val working by model.dataBusy.collectAsState()
    val result by model.dataResult.collectAsState()
    var storage by remember { mutableStateOf<StorageInfo?>(null) }
    // What the library holds, read again whenever its books change or a data operation ends.
    LaunchedEffect(all, working) { storage = runCatching { model.storageInfo() }.getOrNull() }

    fun work(block: suspend () -> ForgeAlert<Nothing>?) {
        model.runDataWork(::failureAlert, block)
    }

    val saveBackup = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/zip")) { uri ->
        if (uri != null) work {
            saveTo(context, uri) { model.backupLibrary() }
            done("Backup Saved", "Restore it from Settings on this phone or another.")
        }
    }
    val saveSpreadsheet = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/csv")) { uri ->
        if (uri != null) work {
            saveTo(context, uri) { model.exportBooksCsv() }
            done("Spreadsheet Saved", null)
        }
    }
    val pickBackup = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            alert = ForgeAlert.confirm(
                "Replace your library with this backup?",
                message = "Everything in the library now is replaced by the backup's books, notes, history, covers and EPUBs.",
                confirm = "Restore",
                action = SettingsAction.Restore(uri),
                identifier = "settings.restoreConfirm",
                destructive = true,
            )
        }
    }
    val pickSpreadsheet = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) work {
            val summary = readFrom(context, uri, "PocketLibrary.csv") { model.importBooksCsv(it) }
            importSummary(summary)
        }
    }

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
        Column(
            Modifier
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Forge.spacing.l.dp)
                .padding(bottom = Forge.spacing.l.dp),
        ) {
            if (working) LinearProgressIndicator(Modifier.fillMaxWidth().testTag("settings.working"))

            Section("Preferences")
            SortRow(settings.defaultSort) { sort ->
                model.finish {
                    try {
                        model.setDefaultSort(sort)
                    } catch (e: Exception) {
                        if (e is CancellationException) throw e
                        alert = failureAlert(e)
                    }
                }
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .toggleable(value = settings.catalogueEnabled, role = Role.Switch) { enabled ->
                        model.finish {
                            try {
                                model.setCatalogueEnabled(enabled)
                            } catch (e: Exception) {
                                if (e is CancellationException) throw e
                                alert = failureAlert(e)
                            }
                        }
                    }
                    .testTag("settings.onlineCatalogue")
                    .padding(vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Online catalogue search", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                Switch(checked = settings.catalogueEnabled, onCheckedChange = null)
            }
            Footer(
                "When online search is off, Pocket Library never connects to the internet except to show covers of books you added from the catalogue before.",
            )

            Section("Your Data")
            storage?.let { info ->
                Value("Books", "${info.books}", "settings.books")
                Value("History entries", "${info.activityEntries}", "settings.historyEntries")
                Value("Saved covers", "${info.savedCovers}", "settings.savedCovers")
                Value("Saved EPUBs", "${info.savedBooks}", "settings.savedBooks")
                Value("Space used", Formatter.formatShortFileSize(context, info.bytes.toLong()), "settings.spaceUsed")
                HorizontalDivider(Modifier.padding(vertical = 4.dp))
            }
            Action("Back Up Library", "settings.export", enabled = !working) { saveBackup.launch("PocketLibrary-backup.zip") }
            Action("Restore from Backup…", "settings.restore", enabled = !working) {
                // Some file managers label a zip as plain binary; Rust checks what the file is.
                pickBackup.launch(arrayOf("application/zip", "application/x-zip-compressed", "application/octet-stream"))
            }
            Action("Export as Spreadsheet", "settings.exportCSV", enabled = !working) { saveSpreadsheet.launch("PocketLibrary.csv") }
            Action("Import from Spreadsheet…", "settings.importCSV", enabled = !working) {
                pickSpreadsheet.launch(arrayOf("text/csv", "text/comma-separated-values", "application/csv", "text/plain"))
            }
            Action("Erase Library…", "settings.erase", enabled = !working, colour = MaterialTheme.colorScheme.error) {
                alert = ForgeAlert.confirm(
                    "Erase your whole library?",
                    message = "All books, notes, history and saved covers are deleted from this phone. This can't be undone.",
                    confirm = "Erase Library",
                    action = SettingsAction.Erase,
                    identifier = "settings.erase",
                    destructive = true,
                )
            }
            Footer(
                "Your library is stored only on this phone. Back it up to keep a copy elsewhere: the backup has your books, notes, history, covers and EPUBs, and restores on this phone or another. A spreadsheet holds your books only, and opens in any spreadsheet app.",
            )

            Section("About")
            Value("Book data", "Open Library", "settings.bookData")
            Value("Built with", "Forge (Rust + Jetpack Compose)", "settings.builtWith")
        }
    }
    ForgeAlertDialog(result, onDismiss = model::clearDataResult)
    ForgeAlertDialog(alert, onDismiss = { alert = null }) { action ->
        when (action) {
            SettingsAction.Erase -> work {
                model.eraseLibrary()
                null
            }
            is SettingsAction.Restore -> work {
                val books = readFrom(context, action.uri, "PocketLibrary-backup.zip") { model.restoreLibrary(it) }
                done("Library restored", if (books == 1u) "1 book" else "$books books")
            }
        }
    }
}

@Composable
private fun Section(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier
            .padding(top = Forge.spacing.l.dp, bottom = 4.dp)
            .semantics { heading() },
    )
}

@Composable
private fun Footer(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(top = 4.dp),
    )
}

/** A label and its value on one line (SwiftUI's LabeledContent). */
@Composable
private fun Value(label: String, value: String, tag: String) {
    Row(Modifier.fillMaxWidth().testTag(tag).padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun Action(title: String, tag: String, enabled: Boolean, colour: Color = Color.Unspecified, onClick: () -> Unit) {
    Text(
        title,
        style = MaterialTheme.typography.bodyLarge,
        color = when {
            !enabled -> MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
            colour != Color.Unspecified -> colour
            else -> MaterialTheme.colorScheme.primary
        },
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .testTag(tag)
            .padding(vertical = 12.dp),
    )
}

/** The library's order, as a menu: `settings.sort`, with each choice `settings.sort.<order>`. */
@Composable
private fun SortRow(sort: LibrarySort, onSelect: (LibrarySort) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .clickable(role = Role.DropdownList) { open = true }
                .testTag("settings.sort")
                .padding(vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Sort library by", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(LibraryModel.sortLabel(sort), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Icon(Icons.Filled.ArrowDropDown, contentDescription = null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (choice in LibrarySort.entries) {
                DropdownMenuItem(
                    text = { Text(LibraryModel.sortLabel(choice)) },
                    leadingIcon = { if (choice == sort) Icon(Icons.Filled.Check, contentDescription = "Selected") },
                    onClick = {
                        open = false
                        if (choice != sort) onSelect(choice)
                    },
                    modifier = Modifier.testTag("settings.sort.${choice.key()}"),
                )
            }
        }
    }
}

private fun done(title: String, message: String?): ForgeAlert<Nothing> =
    ForgeAlert(title, message, listOf(ForgeAlertButton.ok()))

private fun failureAlert(e: Exception): ForgeAlert<Nothing>? = when (e) {
    is IOException -> ForgeAlert.failure(ForgeFailure("Can't Use That File", "Pocket Library couldn't read or write the file you chose."))
    else -> ForgeAlert.failure(e)
}

/** "N books added", with the rows that were skipped, as the iPhone app says it. */
internal fun importSummary(summary: CsvImportSummary): ForgeAlert<Nothing> {
    val added = if (summary.added == 1u) "1 book added" else "${summary.added} books added"
    val skipped = summary.skipped
    val message = if (skipped.isEmpty()) {
        null
    } else {
        val shown = skipped.take(5).joinToString("\n")
        val more = if (skipped.size > 5) "\nand ${skipped.size - 5} more" else ""
        "${skipped.size} rows skipped:\n$shown$more"
    }
    return ForgeAlert(added, message, listOf(ForgeAlertButton.ok()))
}

/**
 * Has Rust write a file ([write] returns its path) and copies it into the document the person
 * created. If that fails, the empty document is removed again.
 */
private suspend fun saveTo(context: Context, uri: Uri, write: suspend () -> String) {
    try {
        val path = write()
        withContext(Dispatchers.IO) {
            val output = context.contentResolver.openOutputStream(uri, "wt") ?: throw IOException("the document could not be opened")
            output.use { out -> File(path).inputStream().use { it.copyTo(out) } }
        }
    } catch (e: Throwable) {
        withContext(NonCancellable + Dispatchers.IO) {
            runCatching { DocumentsContract.deleteDocument(context.contentResolver, uri) }
        }
        throw e
    }
}

/**
 * Copies the document the person picked to a scratch file Rust can read, named [name], runs
 * [read] on it, and deletes the copy.
 */
private suspend fun <T> readFrom(context: Context, uri: Uri, name: String, read: suspend (String) -> T): T {
    val folder = File(context.cacheDir, "settings/${UUID.randomUUID()}")
    try {
        val file = withContext(Dispatchers.IO) {
            if (!folder.mkdirs()) throw IOException("no scratch folder")
            val file = File(folder, name)
            val input = context.contentResolver.openInputStream(uri) ?: throw IOException("the document could not be opened")
            input.use { source -> file.outputStream().use { source.copyTo(it) } }
            file
        }
        return read(file.path)
    } finally {
        withContext(NonCancellable + Dispatchers.IO) { folder.deleteRecursively() }
    }
}
