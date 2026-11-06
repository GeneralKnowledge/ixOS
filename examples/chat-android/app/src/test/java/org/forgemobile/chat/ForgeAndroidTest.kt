package org.forgemobile.chat

import android.content.ContextWrapper
import org.forgemobile.ffi.ForgeAndroid
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Calls the real JNI entry point in the host build of forge-ffi, so a mismatch between the
 * Kotlin `external fun` and the Rust export (class, name or signature) fails here rather than
 * on a phone. On the JVM there is nothing to initialise; on Android the same call hands the
 * Context to the certificate verifier.
 */
class ForgeAndroidTest {
    @Test
    fun initBindsTheNativeEntryPointAndIsIdempotent() {
        val context = ContextWrapper(null)
        ForgeAndroid.init(context)
        assertTrue(ForgeAndroid.isInitialised)
        ForgeAndroid.init(context) // a second call is a no-op
        assertTrue(ForgeAndroid.isInitialised)
    }
}
