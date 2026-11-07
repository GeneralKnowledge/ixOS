package FORGE_JAVA_PACKAGE

import androidx.activity.ComponentActivity
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import uniffi.forge_ffi.AppPhase

// Reports the Android lifecycle to Rust. The rules for what each phase means (which work stops in
// the background, what to refresh on resume) live in `rust/src/lib.rs` (`AppCore::set_phase`),
// not here.

/**
 * The app phase an activity lifecycle event means, or `null` for events that change nothing.
 * `ON_STOP` during a configuration change (rotation, dark mode) is not a trip to the background:
 * the activity comes straight back, so work keeps running.
 */
fun appPhaseFor(event: Lifecycle.Event, changingConfigurations: Boolean): AppPhase? =
    when (event) {
        Lifecycle.Event.ON_START -> AppPhase.INACTIVE
        Lifecycle.Event.ON_RESUME -> AppPhase.ACTIVE
        Lifecycle.Event.ON_PAUSE -> AppPhase.INACTIVE
        Lifecycle.Event.ON_STOP -> if (changingConfigurations) null else AppPhase.BACKGROUND
        else -> null
    }

/**
 * Calls [report] with this activity's lifecycle phases from now on. Call it once in `onCreate`.
 * Termination is not reported here: the view model's `onCleared` shuts Rust down when the
 * activity finishes, and Android may kill a background process without any callback.
 */
fun ComponentActivity.reportLifecycle(report: (AppPhase) -> Unit) {
    val activity = this
    lifecycle.addObserver(
        object : DefaultLifecycleObserver {
            private fun send(event: Lifecycle.Event) {
                appPhaseFor(event, activity.isChangingConfigurations)?.let(report)
            }

            override fun onStart(owner: LifecycleOwner) = send(Lifecycle.Event.ON_START)

            override fun onResume(owner: LifecycleOwner) = send(Lifecycle.Event.ON_RESUME)

            override fun onPause(owner: LifecycleOwner) = send(Lifecycle.Event.ON_PAUSE)

            override fun onStop(owner: LifecycleOwner) = send(Lifecycle.Event.ON_STOP)
        }
    )
}
