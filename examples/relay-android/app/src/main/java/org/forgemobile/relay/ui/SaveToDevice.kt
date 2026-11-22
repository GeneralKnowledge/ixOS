// Saving what arrived out of Relay's private folder (the iPhone's received files show in the
// Files app): a file goes where the person picks with the Storage Access Framework (Downloads by
// default), and a folder into a folder they pick, with everything in it. No storage permission:
// the picker grants just that place.
package org.forgemobile.relay.ui

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.webkit.MimeTypeMap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import java.io.File
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.forgemobile.relay.kit.ForgeFailure

/**
 * A function that saves the file or folder at a path to a place the person picks, telling
 * [done] how it went: null when saved, else why not.
 */
@Composable
fun rememberSaveToDevice(done: (ForgeFailure?) -> Unit): (String) -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var pending by remember { mutableStateOf<File?>(null) }

    fun finish(target: Uri?, copy: suspend (File, Uri) -> Unit) {
        val source = pending ?: return
        pending = null
        if (target == null) return
        scope.launch {
            val failure = try {
                withContext(Dispatchers.IO) { copy(source, target) }
                null
            } catch (e: Exception) {
                ForgeFailure("Couldn't Save", "${source.name} couldn't be saved there. Try another place.")
            }
            done(failure)
        }
    }

    val file = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("*/*")) { uri ->
        finish(uri) { source, target -> SaveToDevice.copyFile(context, source, target) }
    }
    val folder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        finish(uri) { source, target -> SaveToDevice.copyFolder(context, source, target) }
    }
    return { path ->
        val source = File(path)
        when {
            !source.exists() -> done(ForgeFailure("Not Found", "The file is no longer on this device."))
            source.isDirectory -> {
                pending = source
                folder.launch(null)
            }
            else -> {
                pending = source
                file.launch(source.name)
            }
        }
    }
}

object SaveToDevice {
    fun copyFile(context: Context, source: File, target: Uri) {
        val out = context.contentResolver.openOutputStream(target) ?: throw IOException("can't write")
        out.use { sink -> source.inputStream().use { it.copyTo(sink) } }
    }

    /** Copies [source] into the picked tree [target] as a new folder of its name. */
    fun copyFolder(context: Context, source: File, target: Uri) {
        val parent = DocumentsContract.buildDocumentUriUsingTree(target, DocumentsContract.getTreeDocumentId(target))
        copyInto(context, source, parent)
    }

    private fun copyInto(context: Context, source: File, parent: Uri) {
        val resolver = context.contentResolver
        if (source.isDirectory) {
            val dir = DocumentsContract.createDocument(resolver, parent, DocumentsContract.Document.MIME_TYPE_DIR, source.name)
                ?: throw IOException("can't make ${source.name}")
            source.listFiles()?.forEach { copyInto(context, it, dir) }
        } else {
            val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(source.extension.lowercase())
                ?: "application/octet-stream"
            val doc = DocumentsContract.createDocument(resolver, parent, mime, source.name)
                ?: throw IOException("can't make ${source.name}")
            copyFile(context, source, doc)
        }
    }
}
