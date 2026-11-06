// Shipped with the generated Kotlin bindings by scripts/generate-bindings.sh.
// Source of truth: crates/forge-ffi/kotlin/. Do not edit the copy in Generated/.
package org.forgemobile.ffi

import android.content.Context

/**
 * One-time Android setup for the Rust core. Call [init] once at startup, before creating any
 * Forge app object (for example first thing in `Activity.onCreate` or `Application.onCreate`).
 *
 * It hands the application [Context] to the HTTPS certificate verifier in Rust, which checks
 * server certificates with Android's own trust store. Without it, `https://` requests fail with
 * a typed "not supported on this host" error that says to call this; `http://` works either way.
 *
 * Calling it again, from any thread, does nothing.
 */
object ForgeAndroid {
    @Volatile private var initialised = false

    /** Whether [init] has succeeded in this process. */
    val isInitialised: Boolean
        get() = initialised

    /**
     * Initialises the Rust core for this process.
     *
     * @throws ForgeAndroidInitException if the certificate verifier cannot be set up (most often:
     *   the `org.rustls:rustls-platform-verifier` Gradle dependency is missing). HTTPS then stays
     *   disabled; nothing falls back to unverified connections.
     */
    @JvmStatic
    fun init(context: Context) {
        if (initialised) return
        synchronized(this) {
            if (initialised) return
            // The same library the generated bindings load through JNA; loading it here as
            // well registers the JNI entry point below.
            System.loadLibrary("forge_ffi")
            val problem = nativeInit(context.applicationContext ?: context)
            if (problem != null) throw ForgeAndroidInitException(problem)
            initialised = true
        }
    }

    /** Returns null on success, otherwise why HTTPS is unavailable. */
    @JvmStatic private external fun nativeInit(context: Context): String?
}

/** [ForgeAndroid.init] failed; the message says why and how to fix it. */
class ForgeAndroidInitException(message: String) : RuntimeException(message)
