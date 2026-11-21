// Import EPUB File: the system's document picker, as the iPhone app's file importer. The picked
// document is a content URI Rust can't open, so its bytes are copied to a scratch file under its
// own name (Rust keeps the name as the book's source), imported, and the copy deleted.
package org.forgemobile.pocketlibrary.ui

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import java.io.File
import java.io.IOException
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeFailure

/**
 * Returns what opens the picker. [onImported] gets the new (or newly readable) book's id;
 * [onFailure] the alert for a file that couldn't be imported.
 */
@Composable
internal fun rememberEpubImport(
    model: LibraryModel,
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
        scope.launch {
            importing(true)
            // A scratch folder of its own, made here so it is deleted however the copy ends.
            val folder = File(context.cacheDir, "import/${System.nanoTime()}")
            try {
                val copy = withContext(Dispatchers.IO) { copyForImport(context, uri, folder) }
                imported(model.importEpub(copy.path).id)
            } catch (e: IOException) {
                failed(ForgeAlert.failure(ForgeFailure("Can't Read That File", "Pocket Library couldn't read the file you chose.")))
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                failed(ForgeAlert.failure(e))
            } finally {
                withContext(NonCancellable + Dispatchers.IO) { folder.deleteRecursively() }
                importing(false)
            }
        }
    }
    // Some file managers label EPUBs as plain binary files; Rust checks what the file really is.
    return { picker.launch(arrayOf("application/epub+zip", "application/octet-stream")) }
}

/** Copies the picked document into [folder], under its own name. */
private fun copyForImport(context: Context, uri: Uri, folder: File): File {
    val resolver = context.contentResolver
    val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) cursor.getString(0) else null
    }
    val safeName = name?.substringAfterLast('/')?.takeIf { it.isNotBlank() && it != "." && it != ".." } ?: "book.epub"
    if (!folder.mkdirs()) throw IOException("no scratch folder for the import")
    val file = File(folder, safeName)
    val input = resolver.openInputStream(uri) ?: throw IOException("the document could not be opened")
    input.use { source -> file.outputStream().use { source.copyTo(it) } }
    return file
}
