package org.forgemobile.relay

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.forgemobile.relay.host.KeystoreSecureStore
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith

/** The Keystore-backed secure store Rust keeps the device key and account in. */
@RunWith(AndroidJUnit4::class)
class SecureStoreTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun aValueIsSealedReadBackAndRemoved() {
        val store = KeystoreSecureStore(context)
        val key = "test.secure-store"
        val value = ByteArray(32) { it.toByte() }
        store.set(key, value)
        // A new instance reads it, as the next launch would.
        assertArrayEquals(value, KeystoreSecureStore(context).get(key))
        store.set(key, byteArrayOf(1, 2, 3))
        assertArrayEquals(byteArrayOf(1, 2, 3), store.get(key))
        store.delete(key)
        assertNull(store.get(key))
        // Removing a missing key is not an error.
        store.delete(key)
    }

    @Test
    fun aThrowawayStoreNeverTouchesTheRealOne() {
        val store = KeystoreSecureStore(context)
        val key = "test.throwaway"
        store.set(key, byteArrayOf(9))
        store.throwaway = mutableMapOf()
        assertNull(store.get(key))
        store.set(key, byteArrayOf(7))
        store.throwaway = null
        assertArrayEquals(byteArrayOf(9), store.get(key))
        store.delete(key)
    }
}
