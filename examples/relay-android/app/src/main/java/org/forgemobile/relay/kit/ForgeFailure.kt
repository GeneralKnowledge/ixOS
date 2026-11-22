// The Android counterparts of ForgeAppKit's model types (forgekit/Sources/ForgeAppKit/Model).
// Plain Kotlin and coroutines, no Android UI, so JVM tests drive them. They live in this app
// until a second Android app needs them (Apps first, docs/roadmap.md#android--work-in-progress).
package org.forgemobile.relay.kit

import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import kotlinx.coroutines.CancellationException

/** What went wrong, in words a person understands (ForgeFailure in Swift). */
data class ForgeFailure(
    val title: String,
    val message: String,
    /** Whether trying the same thing again might work; failure alerts then offer Try Again. */
    val isRetryable: Boolean = false,
) {
    companion object {
        /**
         * The app's own words for its errors. Kotlin can't add an interface to a generated
         * error class, so the app installs one describer at launch instead of conforming its
         * errors to `ForgeDescribedError` as Swift does.
         */
        @Volatile
        var describer: ForgeErrorDescriber? = null

        fun from(error: Throwable): ForgeFailure =
            describer?.failure(error)
                ?: network(error)
                ?: ForgeFailure(
                    title = "Something Went Wrong",
                    message = "Try again. If it keeps happening, close the app and open it again.",
                )

        /** A cancelled task is not a failure: nothing is shown for it. */
        fun isCancellation(error: Throwable): Boolean =
            error is CancellationException || describer?.isCancellation(error) == true

        private fun network(error: Throwable): ForgeFailure? = when (error) {
            is SocketTimeoutException -> ForgeFailure(
                "No Response",
                "The server took too long to answer. Try again in a moment.",
                isRetryable = true,
            )
            is UnknownHostException, is ConnectException -> ForgeFailure(
                "Can't Reach the Server",
                "Check your connection, then try again.",
                isRetryable = true,
            )
            else -> null
        }
    }
}

/** Gives an app's errors their words (ForgeDescribedError in Swift). */
interface ForgeErrorDescriber {
    /** The failure for [error], or null when it isn't one of the app's errors. */
    fun failure(error: Throwable): ForgeFailure?

    /** Whether [error] means the work was cancelled rather than failed. */
    fun isCancellation(error: Throwable): Boolean = false
}
