// The Android form of forge-services' biometrics adapter (ForgeServices+Biometrics.swift):
// fingerprint, face or the screen lock, through androidx.biometric's BiometricPrompt.
package org.forgemobile.relay.host

import android.app.KeyguardManager
import android.content.Context
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_WEAK
import androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.relay_core.AuthenticationResult
import uniffi.relay_core.BiometricAuthenticator
import uniffi.relay_core.BiometricAvailability
import uniffi.relay_core.BiometryKind

/**
 * `BiometricAuthenticator` over BiometricPrompt, which needs the activity on screen: it
 * [attach]es itself while it is. Android names no single sensor, so the kind is always `NONE`
 * and the screens say "screen lock" rather than Rust's iPhone names.
 */
class AndroidBiometrics(private val context: Context) : BiometricAuthenticator {
    private val activity = MutableStateFlow<FragmentActivity?>(null)

    /** The activity on screen; call again with null when it goes. */
    fun attach(activity: FragmentActivity?) {
        this.activity.value = activity
    }

    override fun availability(): BiometricAvailability {
        val biometrics = BiometricManager.from(context).canAuthenticate(BIOMETRIC_WEAK) == BiometricManager.BIOMETRIC_SUCCESS
        val screenLock = context.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true
        return BiometricAvailability(kind = BiometryKind.NONE, biometricsUsable = biometrics, passcodeSet = screenLock)
    }

    /** Whether a lock could open at all: a fingerprint or face, or the screen lock. */
    val canLock: Boolean get() = availability().let { it.biometricsUsable || it.passcodeSet }

    override suspend fun authenticate(reason: String): AuthenticationResult {
        if (!canLock) return AuthenticationResult.UNAVAILABLE
        // Asked from the background (a notification), nothing can show the prompt.
        val shown = withTimeoutOrNull(ACTIVITY_WAIT_MS) { activity.filterNotNull().first() }
            ?: return AuthenticationResult.CANCELLED
        return withContext(Dispatchers.Main) {
            suspendCancellableCoroutine { done ->
                val prompt = BiometricPrompt(
                    shown,
                    ContextCompat.getMainExecutor(shown),
                    object : BiometricPrompt.AuthenticationCallback() {
                        override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                            if (done.isActive) done.resume(AuthenticationResult.AUTHENTICATED)
                        }

                        override fun onAuthenticationError(code: Int, message: CharSequence) {
                            if (done.isActive) done.resume(result(code))
                        }
                        // onAuthenticationFailed is one wrong finger: the prompt stays up.
                    },
                )
                val info = BiometricPrompt.PromptInfo.Builder()
                    .setTitle(reason)
                    // The screen lock as well as biometrics, so a phone with only a PIN can lock.
                    .setAllowedAuthenticators(BIOMETRIC_WEAK or DEVICE_CREDENTIAL)
                    .build()
                prompt.authenticate(info)
                done.invokeOnCancellation { prompt.cancelAuthentication() }
            }
        }
    }

    companion object {
        private const val ACTIVITY_WAIT_MS = 5_000L

        /** BiometricPrompt's error codes in forge-services' words. */
        fun result(code: Int): AuthenticationResult = when (code) {
            BiometricPrompt.ERROR_USER_CANCELED,
            BiometricPrompt.ERROR_NEGATIVE_BUTTON,
            BiometricPrompt.ERROR_CANCELED,
            -> AuthenticationResult.CANCELLED
            BiometricPrompt.ERROR_LOCKOUT,
            BiometricPrompt.ERROR_LOCKOUT_PERMANENT,
            -> AuthenticationResult.LOCKED_OUT
            BiometricPrompt.ERROR_NO_BIOMETRICS,
            BiometricPrompt.ERROR_HW_NOT_PRESENT,
            BiometricPrompt.ERROR_HW_UNAVAILABLE,
            BiometricPrompt.ERROR_NO_DEVICE_CREDENTIAL,
            -> AuthenticationResult.UNAVAILABLE
            else -> AuthenticationResult.FAILED
        }
    }
}
