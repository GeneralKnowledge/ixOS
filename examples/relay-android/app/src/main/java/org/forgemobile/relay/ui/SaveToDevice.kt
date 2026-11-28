// Saving what arrived out of Relay's private folder (the iPhone's received files show in the
// Files app): a file goes where the person picks with the Storage Access Framework (Downloads by
// default), and a folder into a folder they pick, with everything in it. No storage permission:
// the picker grants just that place.
package org.forgemobile.relay.ui

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.webkit.MimeTypeMap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContract
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
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
 * [done] how it went: null when saved, else why not. A file is offered under `name` (its own
 * name when null), with `mime` as the picker's type.
 */
@Composable
fun rememberSaveToDevice(done: (ForgeFailure?) -> Unit): (path: String, name: String?, mime: String?) -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // Its path, saved with the screen: a rotation while the picker is open keeps it.
    var pending by rememberSaveable { mutableStateOf<String?>(null) }
    var pendingName by rememberSaveable { mutableStateOf<String?>(null) }

    fun finish(target: Uri?, copy: suspend (File, Uri) -> Unit) {
        val source = pending?.let(::File) ?: return
        val shown = pendingName ?: source.name
        pending = null
        pendingName = null
        if (target == null) return
        scope.launch {
            val failure = try {
                withContext(Dispatchers.IO) { copy(source, target) }
                null
            } catch (e: Exception) {
                ForgeFailure("Couldn't Save", "$shown couldn't be saved there. Try another place.")
            }
            done(failure)
        }
    }

    val file = rememberLauncherForActivityResult(CreateTypedDocument()) { uri ->
        finish(uri) { source, target -> SaveToDevice.copyFile(context, source, target) }
    }
    val folder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        finish(uri) { source, target -> SaveToDevice.copyFolder(context, source, target) }
    }
    return { path, name, mime ->
        val source = File(path)
        when {
            !source.exists() -> done(ForgeFailure("Not Found", "The file is no longer on this device."))
            source.isDirectory -> {
                pending = path
                pendingName = name
                folder.launch(null)
            }
            else -> {
                pending = path
                pendingName = name
                val type = mime?.takeIf { it.contains('/') && it != "application/octet-stream" } ?: "*/*"
                file.launch((name ?: source.name) to type)
            }
        }
    }
}

/** Create Document with the type given at launch (`CreateDocument` fixes it when remembered). */
private class CreateTypedDocument : ActivityResultContract<Pair<String, String>, Uri?>() {
    override fun createIntent(context: Context, input: Pair<String, String>): Intent =
        Intent(Intent.ACTION_CREATE_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType(input.second)
            .putExtra(Intent.EXTRA_TITLE, input.first)

    override fun parseResult(resultCode: Int, intent: Intent?): Uri? =
        intent.takeIf { resultCode == Activity.RESULT_OK }?.data
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
            // Null is a failure to list it (an empty folder is an empty array): never a
            // partial copy reported as saved.
            val children = source.listFiles() ?: throw IOException("can't list ${source.name}")
            children.forEach { copyInto(context, it, dir) }
        } else {
            val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(source.extension.lowercase())
                ?: "application/octet-stream"
            val doc = DocumentsContract.createDocument(resolver, parent, mime, source.name)
                ?: throw IOException("can't make ${source.name}")
            copyFile(context, source, doc)
        }
    }
}
