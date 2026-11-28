// The Android form of forge-services' background adapter (ForgeServices+Background.swift):
// Rust's background tasks run as WorkManager work, where iOS uses BGTaskScheduler. Rust
// registers the handlers and asks again after each run; this only schedules and runs them.
package org.forgemobile.relay.host

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CompletableDeferred
import uniffi.relay_core.BackgroundScheduler
import uniffi.relay_core.BackgroundTaskCompletion
import uniffi.relay_core.BackgroundTaskKind
import uniffi.relay_core.BackgroundTaskRequest
import uniffi.relay_core.NativeServiceException
import uniffi.relay_core.runNativeBackgroundTask

/**
 * `BackgroundScheduler` over WorkManager. [permitted] is the app's list of task identifiers
 * (forge.toml `[ios] background_tasks`, which iOS needs in Info.plist).
 *
 * Rust submits a task's next run at the start of each run. WorkManager would cancel the
 * running work if it were replaced under its own name, so each task alternates between two
 * unique names: the next run is queued under the one the current run isn't using.
 */
class WorkManagerScheduler(private val context: Context, private val permitted: List<String>) : BackgroundScheduler {
    override fun submit(request: BackgroundTaskRequest) {
        val identifier = request.identifier
        if (identifier !in permitted) {
            throw NativeServiceException.InvalidInput("background task \"$identifier\" is not in forge.toml [ios] background_tasks")
        }
        val constraints = Constraints.Builder()
        when (val kind = request.kind) {
            // A refresh fetches new content: no point without a network.
            is BackgroundTaskKind.Refresh -> constraints.setRequiredNetworkType(NetworkType.CONNECTED)
            is BackgroundTaskKind.Processing -> {
                if (kind.requiresNetwork) constraints.setRequiredNetworkType(NetworkType.CONNECTED)
                constraints.setRequiresCharging(kind.requiresExternalPower)
            }
        }
        val running = ForgeBackgroundWorker.running[identifier]
        val name = names(identifier).first { it != running }
        val work = OneTimeWorkRequestBuilder<ForgeBackgroundWorker>()
            .setInitialDelay(request.earliestBeginSeconds.toLong().coerceAtLeast(0), TimeUnit.SECONDS)
            .setConstraints(constraints.build())
            .setInputData(workDataOf(ForgeBackgroundWorker.IDENTIFIER to identifier, ForgeBackgroundWorker.NAME to name))
            .addTag(TAG)
            .build()
        val manager = WorkManager.getInstance(context)
        manager.enqueueUniqueWork(name, ExistingWorkPolicy.REPLACE, work)
        // One waiting run per task, as on iOS: a request replaces the earlier one.
        if (running == null) names(identifier).filter { it != name }.forEach(manager::cancelUniqueWork)
    }

    override fun cancel(identifier: String) {
        val manager = WorkManager.getInstance(context)
        names(identifier).filter { it != ForgeBackgroundWorker.running[identifier] }.forEach(manager::cancelUniqueWork)
    }

    override fun permitted(): List<String> = permitted

    private fun names(identifier: String) = listOf(identifier, "$identifier#next")

    private companion object {
        const val TAG = "forge-background"
    }
}

/** One run of a background task: opens the app's Rust, then hands Rust the window. */
class ForgeBackgroundWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val identifier = inputData.getString(IDENTIFIER) ?: return Result.failure()
        val name = inputData.getString(NAME) ?: identifier
        running[identifier] = name
        try {
            // Started with the app closed: its cores open first, so Rust has the task's handler.
            val open = prepare
            if (open != null && !open()) return Result.failure()
            val finished = CompletableDeferred<Boolean>()
            val run = runNativeBackgroundTask(
                identifier,
                object : BackgroundTaskCompletion {
                    override fun finished(success: Boolean) {
                        finished.complete(success)
                    }
                },
            )
            return try {
                if (finished.await()) Result.success() else Result.failure()
            } catch (e: CancellationException) {
                // WorkManager took the time back (constraints lost, or its 10 minutes ran out):
                // Rust's handler stops, as when iOS ends the window.
                run.expire()
                throw e
            } finally {
                run.close()
            }
        } finally {
            running.remove(identifier, name)
        }
    }

    companion object {
        const val IDENTIFIER = "forge.identifier"
        const val NAME = "forge.name"

        /** The unique name each task's current run was queued under. */
        internal val running = ConcurrentHashMap<String, String>()

        /**
         * Set by the app: opens its Rust cores if they aren't, returning true once they are
         * (false if they couldn't open).
         */
        @Volatile
        var prepare: (suspend () -> Boolean)? = null
    }
}
