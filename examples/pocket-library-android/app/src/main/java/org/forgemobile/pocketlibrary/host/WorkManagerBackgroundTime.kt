// Background time on Android: the counterpart of ForgeKit's UIKit `beginBackgroundTask`. While
// Rust has work to finish after the app leaves the screen, a WorkManager job runs; a process
// with a running job is not frozen or ended as an idle cached app would be. The job ends when
// Rust's work does or the app comes back. If the system stops it first (its time limit, or the
// person stopping the app's work), the host tells Rust, which stops its finishing work cleanly.
package org.forgemobile.pocketlibrary.host

import android.content.Context
import android.os.Build
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import java.util.UUID
import kotlinx.coroutines.CompletableDeferred

class WorkManagerBackgroundTime(context: Context) : ForgeBackgroundTime {
    private val context = context.applicationContext

    override fun begin(onExpiry: () -> Unit) {
        val hold = Hold(UUID.randomUUID().toString(), onExpiry)
        current = hold
        val request = OneTimeWorkRequestBuilder<BackgroundTimeWorker>()
            .setInputData(workDataOf(HOLD to hold.id))
            .apply {
                // Expedited from Android 12, where it is a job of its own that starts at once.
                // Earlier, expedited work runs as a foreground service with a notification;
                // ordinary work, which starts as soon as the system allows, is enough there.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                }
            }
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(WORK_NAME, ExistingWorkPolicy.REPLACE, request)
    }

    override fun end() {
        current?.released?.complete(Unit)
        current = null
        WorkManager.getInstance(context).cancelUniqueWork(WORK_NAME)
    }

    /** One [begin]: released by [end], or expired when the system stops the job first. */
    internal class Hold(val id: String, val onExpiry: () -> Unit) {
        val released = CompletableDeferred<Unit>()
    }

    companion object {
        internal const val WORK_NAME = "forge.backgroundTime"
        internal const val HOLD = "hold"

        /** The hold this process has now; the worker waits on it. */
        @Volatile
        internal var current: Hold? = null
    }
}

/** Runs while the host holds background time. WorkManager makes it from its class name. */
class BackgroundTimeWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val hold = WorkManagerBackgroundTime.current
        // A hold from a process that has since ended (WorkManager runs the job again in the new
        // one), or one already released: there is nothing to finish.
        if (hold == null || hold.id != inputData.getString(WorkManagerBackgroundTime.HOLD)) return Result.success()
        try {
            hold.released.await()
        } finally {
            // Stopped by the system before the host released it: the time ran out.
            if (!hold.released.isCompleted) hold.onExpiry()
        }
        return Result.success()
    }
}
