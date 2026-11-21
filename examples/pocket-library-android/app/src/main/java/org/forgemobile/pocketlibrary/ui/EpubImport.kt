// Import EPUB File: the system's document picker, as the iPhone app's file importer. The picked
// document is a content URI Rust can't open, so EpubImports copies it beside the library and a
// WorkManager job imports it: leaving the app, or the process ending, doesn't lose the import.
package org.forgemobile.pocketlibrary.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import java.io.IOException
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.PocketLibraryApplication
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeFailure

/**
 * Returns what opens the picker. [onImported] gets the new (or newly readable) book's id;
 * [onFailure] the alert for a file that couldn't be imported.
 */
@Composable
internal fun rememberEpubImport(
    onImported: (ULong) -> Unit,
    onFailure: (ForgeAlert<Nothing>?) -> Unit,
    onImporting: (Boolean) -> Unit,
): () -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    val imported by rememberUpdatedState(onImported)
    val failed by rememberUpdatedState(onFailure)
    val importing by rememberUpdatedState(onImporting)
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val imports = (context.applicationContext as PocketLibraryApplication).imports
        scope.launch {
            importing(true)
            try {
                imported(imports.import(uri).id)
            } catch (e: IOException) {
                failed(ForgeAlert.failure(ForgeFailure("Can't Read That File", "Pocket Library couldn't read the file you chose.")))
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                failed(ForgeAlert.failure(e))
            } finally {
                importing(false)
            }
        }
    }
    // Some file managers label EPUBs as plain binary files; Rust checks what the file really is.
    return { picker.launch(arrayOf("application/epub+zip", "application/octet-stream")) }
}
