package org.forgemobile.relay.kit

import androidx.biometric.BiometricPrompt
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.yield
import org.forgemobile.relay.host.AndroidBiometrics
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.relay_core.AuthenticationResult

class ForgeAppLockStateTest {
    private var now = 0L
    private val state = ForgeAppLockState(graceMillis = 60_000, clock = { now })

    private var lockable = true
    private val canLock = { lockable }

    private suspend fun unlocked() = state.unlock(canLock) { AuthenticationResult.AUTHENTICATED }

    @Test
    fun startsLockedAndOpensOnSuccess() = runTest {
        assertTrue(state.locked.value)
        unlocked()
        assertFalse(state.locked.value)
        assertNull(state.message.value)
    }

    @Test
    fun aShortTripAwayKeepsItOpen() = runTest {
        unlocked()
        state.left()
        now += 60_000
        state.returned()
        assertFalse(state.locked.value)
    }

    @Test
    fun moreThanTheGracePeriodLocksIt() = runTest {
        unlocked()
        state.left()
        now += 60_001
        state.returned()
        assertTrue(state.locked.value)
    }

    @Test
    fun thePromptItselfIsNotLeaving() = runTest {
        // BiometricPrompt can take the app off screen; that is the unlock, not a trip away.
        unlocked()
        state.returned() // nothing recorded: a return with no leave changes nothing
        assertFalse(state.locked.value)

        val asked = CompletableDeferred<AuthenticationResult>()
        state.left()
        now += 120_000
        state.returned()
        assertTrue(state.locked.value)
        val job = launch { state.unlock(canLock) { asked.await() } }
        yield()
        assertTrue(state.authenticating.value)
        state.left()
        now += 120_000
        state.returned()
        asked.complete(AuthenticationResult.AUTHENTICATED)
        job.join()
        assertFalse(state.locked.value)
    }

    @Test
    fun aSecondUnlockWhileAskingDoesNothing() = runTest {
        var asks = 0
        val asked = CompletableDeferred<AuthenticationResult>()
        val first = launch { state.unlock(canLock) { asks++; asked.await() } }
        yield()
        state.unlock(canLock) { asks++; AuthenticationResult.AUTHENTICATED }
        assertEquals(1, asks)
        asked.complete(AuthenticationResult.CANCELLED)
        first.join()
        assertTrue(state.locked.value)
        assertFalse(state.authenticating.value)
    }

    @Test
    fun lockedOutSaysSoAndStaysLocked() = runTest {
        state.unlock(canLock) { AuthenticationResult.LOCKED_OUT }
        assertTrue(state.locked.value)
        assertEquals("Too many attempts. Try again later, or use your screen lock.", state.message.value)
        state.unlock(canLock) { AuthenticationResult.CANCELLED }
        assertTrue(state.locked.value)
        assertNull(state.message.value)
    }

    @Test
    fun noScreenLockAtAllOpensIt() = runTest {
        lockable = false
        state.unlock(canLock) { AuthenticationResult.UNAVAILABLE }
        assertFalse(state.locked.value)
    }

    @Test
    fun aBusySensorOnALockedPhoneStaysLocked() = runTest {
        // ERROR_HW_UNAVAILABLE ("try again later") is UNAVAILABLE too: never a way in.
        state.unlock(canLock) { AuthenticationResult.UNAVAILABLE }
        assertTrue(state.locked.value)
        assertEquals("Can't ask right now. Try again in a moment.", state.message.value)
        unlocked()
        assertFalse(state.locked.value)
    }

    @Test
    fun changingTheSettingNeverLocksTheOpenApp() = runTest {
        state.settingChanged()
        assertFalse(state.locked.value)
        state.left()
        now += 30_000
        state.settingChanged()
        now += 60_000
        state.returned()
        assertFalse(state.locked.value)
    }

    @Test
    fun promptErrorsInForgeWords() {
        assertEquals(AuthenticationResult.CANCELLED, AndroidBiometrics.result(BiometricPrompt.ERROR_USER_CANCELED))
        assertEquals(AuthenticationResult.CANCELLED, AndroidBiometrics.result(BiometricPrompt.ERROR_NEGATIVE_BUTTON))
        assertEquals(AuthenticationResult.LOCKED_OUT, AndroidBiometrics.result(BiometricPrompt.ERROR_LOCKOUT))
        assertEquals(AuthenticationResult.LOCKED_OUT, AndroidBiometrics.result(BiometricPrompt.ERROR_LOCKOUT_PERMANENT))
        assertEquals(AuthenticationResult.UNAVAILABLE, AndroidBiometrics.result(BiometricPrompt.ERROR_NO_DEVICE_CREDENTIAL))
        assertEquals(AuthenticationResult.FAILED, AndroidBiometrics.result(BiometricPrompt.ERROR_TIMEOUT))
    }
}
