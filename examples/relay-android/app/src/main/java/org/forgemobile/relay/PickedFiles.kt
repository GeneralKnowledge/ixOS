// Files the person picks (the Storage Access Framework or the photo picker) come as content
// URIs, which Rust can't read: each is copied into the app's cache first, under its own name, as
// the iPhone app's ForgePickedFile copies a picked file in. Rust copies what it keeps into its own
// store, so the copies go once the next send starts.
package org.forgemobile.relay

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import java.io.File
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext

class PickedFiles(private val context: Context, private val folder: String) {
    private val root: File get() = File(context.cacheDir, folder)

    /**
     * Copies [uris] in, off the main thread. Throws when one can't be read. A failure or a
     * cancellation (the screen closing, even as the copies come back) removes the whole batch.
     */
    suspend fun copy(uris: List<Uri>): List<String> {
        val batch = File(root, UUID.randomUUID().toString())
        return try {
            withContext(Dispatchers.IO) { copyInto(batch, uris) }
        } catch (e: Exception) {
            withContext(NonCancellable + Dispatchers.IO) { batch.deleteRecursively() }
            throw e
        }
    }

    private suspend fun copyInto(batch: File, uris: List<Uri>): List<String> =
        uris.mapIndexed { index, uri ->
            // A closed screen stops before the next file, not after the last.
            currentCoroutineContext().ensureActive()
            // Each in its own folder, so two picks with one name both keep it.
            val dir = File(batch, index.toString()).apply { mkdirs() }
            val target = File(dir, safeName(displayName(uri) ?: "item-${index + 1}"))
            val input = context.contentResolver.openInputStream(uri)
                ?: throw java.io.IOException("$uri can't be opened")
            input.use { source -> target.outputStream().use { source.copyTo(it) } }
            target.absolutePath
        }

    /**
     * Copies the folder picked with the Storage Access Framework ([tree]) in, with everything
     * in it, and returns where the copy is. Throws when any of it can't be read.
     */
    suspend fun copyTree(tree: Uri): String = withContext(Dispatchers.IO) {
        val batch = File(root, UUID.randomUUID().toString())
        try {
            val id = DocumentsContract.getTreeDocumentId(tree)
            val name = displayName(DocumentsContract.buildDocumentUriUsingTree(tree, id)) ?: "Folder"
            // Under an index folder like [copy]'s, so [release] finds the batch.
            val target = File(File(batch, "0"), safeName(name))
            copyChildren(tree, id, target)
            target.absolutePath
        } catch (e: Exception) {
            batch.deleteRecursively()
            throw e
        }
    }

    private fun copyChildren(tree: Uri, parentId: String, into: File) {
        into.mkdirs()
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId)
        val columns = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        val cursor = context.contentResolver.query(children, columns, null, null, null)
            ?: throw java.io.IOException("the folder can't be listed")
        cursor.use {
            while (it.moveToNext()) {
                val id = it.getString(0)
                val name = safeName(it.getString(1) ?: "item")
                if (it.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
                    copyChildren(tree, id, File(into, name))
                } else {
                    val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                    val input = context.contentResolver.openInputStream(uri)
                        ?: throw java.io.IOException("$name can't be opened")
                    input.use { source -> File(into, name).outputStream().use { source.copyTo(it) } }
                }
            }
        }
    }

    /** Removes every copy. */
    fun release() {
        root.deleteRecursively()
    }

    /**
     * Removes only these copies, given paths [copy] returned, and a pick's folder once none of
     * its copies is left: removing one of a pick's files keeps the others.
     */
    fun release(paths: List<String>) {
        val batches = paths.mapNotNull { path ->
            val item = File(path).parentFile ?: return@mapNotNull null
            val batch = item.parentFile ?: return@mapNotNull null
            if (batch.parentFile != root) return@mapNotNull null
            item.deleteRecursively()
            batch
        }
        batches.distinct().filter { it.list()?.isEmpty() == true }.forEach { it.delete() }
    }

    private fun displayName(uri: Uri): String? =
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) cursor.getString(0) else null
        }

    companion object {
        /** A name that stays one file name: no folders, nothing hidden. */
        fun safeName(name: String): String {
            val base = name.substringAfterLast('/').trim().trimStart('.')
            return base.ifEmpty { "item" }
        }

        /** "photo.jpg" or "3 items". */
        fun summary(names: List<String>): String = if (names.size == 1) names[0] else "${names.size} items"
    }
}
