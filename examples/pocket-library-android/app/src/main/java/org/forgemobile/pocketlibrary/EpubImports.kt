// EPUB imports that finish whatever happens to the app. The picked document is copied next to
// the library (not into the cache, which Android may empty), and a WorkManager job imports it.
// The job is saved by WorkManager, so if the process ends first (the system reclaims memory, the
// person swipes the app away) it runs again the next time the process starts, and the book
// still arrives. Rust's import matches a book by title and author, so a job that runs twice
// can't add a book twice.
package org.forgemobile.pocketlibrary

import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.util.Log
import androidx.work.BackoffPolicy
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import org.forgemobile.pocketlibrary.host.ForgeEnvironment
import org.forgemobile.pocketlibrary.host.ForgeHost
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.LibraryException

class EpubImports(private val app: PocketLibraryApplication) {
    /** Screens waiting for an import in this process, by the import's id. */
    private val waiting = ConcurrentHashMap<String, CompletableDeferred<Book>>()

    /**
     * Test data only (`pocketLibrary.holdImportSeconds`): how long an import's first run waits
     * before it starts, so a test can end the process while it is still to do.
     */
    @Volatile
    var firstRunHold: Long = 0

    /**
     * Imports the document the person picked and returns the book once it is in the library.
     * Cancelling the caller (the screen leaving) doesn't stop the import; the book arrives anyway.
     * Throws an [IOException] when the document can't be read, and the Rust error when the file
     * isn't a book Pocket Library can open.
     */
    suspend fun import(uri: Uri): Book {
        val folder = newFolder() ?: throw LibraryException.ShutDown()
        val file = try {
            withContext(Dispatchers.IO) { copy(app, uri, folder) }
        } catch (e: Throwable) {
            withContext(Dispatchers.IO) { folder.deleteRecursively() }
            throw e
        }
        val result = CompletableDeferred<Book>()
        waiting[folder.name] = result
        try {
            enqueue(file)
            return result.await()
        } finally {
            waiting.remove(folder.name)
        }
    }

    /**
     * Test data only (iOS `-PocketLibrarySampleBook`): once the library is open, and only if it is
     * empty, imports a short sample EPUB, through a job like any import.
     */
    suspend fun addSampleBook() {
        // Only once it is open: opening a throwaway library empties its folder, imports and all.
        app.host.status.first { it == ForgeHost.Status.Ready || it is ForgeHost.Status.Failed }
        val folder = newFolder() ?: return
        val file = File(folder, "Sample Book.epub")
        try {
            withContext(Dispatchers.IO) { folder.mkdirs() }
            if (app.model.writeSampleBook(file)) {
                enqueue(file)
                return
            }
        } catch (e: Exception) {
            if (e is CancellationException) throw e
            Log.w(TAG, "sample book not added: ${e.javaClass.simpleName}")
        }
        withContext(Dispatchers.IO) { folder.deleteRecursively() }
    }

    /** A new folder for one import, beside the open library's data; null while none is open. */
    private fun newFolder(): File? {
        if (app.host.core == null) return null
        val env = app.host.openedEnvironment ?: return null
        return File(pendingFolder(env), UUID.randomUUID().toString())
    }

    private fun enqueue(file: File) {
        val request = OneTimeWorkRequestBuilder<EpubImportWorker>()
            .setInputData(
                workDataOf(
                    EpubImportWorker.PATH to file.path,
                    EpubImportWorker.TEMPORARY to file.path.startsWith(app.cacheDir.path),
                ),
            )
            .setBackoffCriteria(BackoffPolicy.LINEAR, 10, TimeUnit.SECONDS)
            .addTag(TAG_IMPORT)
            .apply {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                }
            }
            .build()
        WorkManager.getInstance(app).enqueueUniqueWork(
            "epub-import-${file.parentFile?.name}",
            ExistingWorkPolicy.KEEP,
            request,
        )
    }

    /** What a job's run came to. */
    internal enum class Outcome { DONE, RETRY }

    /**
     * One run of the job for [file]. [firstRun] is the job's first attempt; a later one may find
     * the import already done (the process ended after Rust saved it).
     */
    internal suspend fun run(file: File, temporary: Boolean, firstRun: Boolean): Outcome {
        val id = file.parentFile?.name ?: return Outcome.DONE
        if (firstRun && firstRunHold > 0) delay(firstRunHold)
        if (!app.openForWork(temporary)) return Outcome.RETRY
        val env = app.host.openedEnvironment ?: return Outcome.RETRY
        // The file went with its library: a throwaway test library that a new launch emptied.
        if (!withContext(Dispatchers.IO) { file.exists() }) return Outcome.DONE
        // Another library is open (a test library while the person's import waits): later.
        if (file.parentFile?.parentFile != pendingFolder(env)) return Outcome.RETRY
        val waiter = waiting[id]
        try {
            val book = app.model.importEpub(file.path)
            waiter?.complete(book)
        } catch (e: LibraryException) {
            when {
                // The library closed, or the background time ran out: nothing changed, so run
                // again.
                e is LibraryException.ShutDown || e is LibraryException.Interrupted || e is LibraryException.Cancelled ->
                    return Outcome.RETRY
                // An earlier run finished just before the process ended.
                e is LibraryException.AlreadyInLibrary && !firstRun -> {
                    val book = app.model.book(e.id)
                    if (book != null) waiter?.complete(book) else waiter?.completeExceptionally(e)
                }
                else -> waiter?.completeExceptionally(e)
            }
        } catch (e: CancellationException) {
            // WorkManager stopped the job; it runs it again.
            throw e
        } catch (e: Exception) {
            waiter?.completeExceptionally(e)
        }
        withContext(Dispatchers.IO) { file.parentFile?.deleteRecursively() }
        return Outcome.DONE
    }

    companion object {
        private const val TAG = "PocketLibrary"
        const val TAG_IMPORT = "epub-import"

        /** Where imports wait: beside the library's data folder, so they go with it. */
        fun pendingFolder(env: ForgeEnvironment): File = File(File(env.dataDirectory).parentFile, "imports")

        /** Copies the picked document into [folder], under its own name (Rust keeps it). */
        fun copy(context: Context, uri: Uri, folder: File): File {
            val resolver = context.contentResolver
            val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) else null
            }
            if (!folder.mkdirs()) throw IOException("no folder for the import")
            val file = File(folder, safeName(name, "book.epub"))
            val input = resolver.openInputStream(uri) ?: throw IOException("the document could not be opened")
            input.use { source -> file.outputStream().use { source.copyTo(it) } }
            return file
        }

        /** A document's name as a file name in a folder of ours, or [fallback]. */
        fun safeName(name: String?, fallback: String): String =
            name?.substringAfterLast('/')?.takeIf { it.isNotBlank() && it != "." && it != ".." } ?: fallback
    }
}

/** Imports one EPUB (see [EpubImports]). WorkManager makes it from its class name. */
class EpubImportWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val app = applicationContext as PocketLibraryApplication
        val path = inputData.getString(PATH) ?: return Result.failure()
        val outcome = app.imports.run(File(path), inputData.getBoolean(TEMPORARY, false), firstRun = runAttemptCount == 0)
        return when (outcome) {
            EpubImports.Outcome.DONE -> Result.success()
            EpubImports.Outcome.RETRY -> Result.retry()
        }
    }

    companion object {
        const val PATH = "path"
        const val TEMPORARY = "temporary"
    }
}
