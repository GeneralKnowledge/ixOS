package org.forgemobile.counter

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import uniffi.forge_ffi.ForgeActionKind
import uniffi.forge_ffi.ForgeAppException
import uniffi.forge_ffi.ForgeCounterApp
import uniffi.forge_ffi.StartupKind
import uniffi.forge_ffi.forgeGreeting
import uniffi.forge_ffi.recoverCounterApp

/**
 * @param storageDirectory where Rust keeps its snapshot. [CounterViewModelFactory] passes a folder
 *   under the app's files dir; tests pass a temporary folder.
 */
class CounterViewModel(private val storageDirectory: String) : ViewModel() {
    var count by mutableStateOf(0L)
        private set
    var startupLabel by mutableStateOf("")
        private set
    var errorMessage by mutableStateOf<String?>(null)
        private set
    var isBlocked by mutableStateOf(false)
        private set
    /** True only for corrupt data. Data saved by a newer app version is never offered for reset. */
    var canRecoverFromCorruption by mutableStateOf(false)
        private set

    val greeting: String = forgeGreeting()

    private var app: ForgeCounterApp? = null

    init {
        openApp()
    }

    fun retryOpen() {
        openApp()
    }

    fun recoverFromCorruption() {
        errorMessage = null
        try {
            val recovered = recoverCounterApp(storageDirectory)
            attach(recovered, afterRecovery = true)
        } catch (e: ForgeAppException) {
            presentBlocked(e)
        } catch (e: Exception) {
            isBlocked = true
            errorMessage = e.message
        }
    }

    fun increment() = apply(ForgeActionKind.INCREMENT)
    fun decrement() = apply(ForgeActionKind.DECREMENT)
    fun reset() = apply(ForgeActionKind.RESET)

    private fun openApp() {
        errorMessage = null
        isBlocked = false
        canRecoverFromCorruption = false
        try {
            val opened = ForgeCounterApp(storageDirectory)
            attach(opened, afterRecovery = false)
        } catch (e: ForgeAppException) {
            presentBlocked(e)
        } catch (e: Exception) {
            isBlocked = true
            errorMessage = e.message
        }
    }

    private fun attach(opened: ForgeCounterApp, afterRecovery: Boolean) {
        val snapshot: Long
        val kind: StartupKind
        try {
            snapshot = opened.snapshot().count
            kind = opened.startupKind()
        } catch (e: ForgeAppException) {
            // Never show a made-up count when Rust could not report the real one.
            opened.close()
            presentBlocked(e)
            return
        }
        app?.close()
        app = opened
        isBlocked = false
        canRecoverFromCorruption = false
        count = snapshot
        startupLabel =
            when {
                afterRecovery && kind == StartupKind.FRESH_INSTALL -> "Recovered — fresh counter"
                kind == StartupKind.FRESH_INSTALL -> "Fresh install"
                else -> "Restored saved state"
            }
    }

    private fun apply(action: ForgeActionKind) {
        val current = app ?: return
        errorMessage = null
        try {
            val outcome = current.apply(action)
            count = outcome.snapshot.count
        } catch (e: ForgeAppException) {
            errorMessage = messageFor(e)
            try {
                count = current.snapshot().count
            } catch (_: ForgeAppException) {
            }
        } catch (e: Exception) {
            errorMessage = e.message
        }
    }

    private fun presentBlocked(error: ForgeAppException) {
        isBlocked = true
        app?.close()
        app = null
        canRecoverFromCorruption = false
        when (error) {
            is ForgeAppException.CorruptPersistence -> {
                canRecoverFromCorruption = true
                errorMessage =
                    "Stored counter data is invalid. You can reset storage or retry opening."
            }
            is ForgeAppException.UnsupportedSchemaVersion ->
                errorMessage =
                    "Your counter was saved by a newer version of this app. " +
                    "Update the app to open it; your data has not been changed."
            is ForgeAppException.PersistenceReadFailed ->
                errorMessage = "Could not read saved data. Check storage access and retry."
            is ForgeAppException.StorageDirectoryUnavailable ->
                errorMessage = "Storage folder is unavailable. Retry after fixing permissions."
            else -> errorMessage = messageFor(error)
        }
    }

    override fun onCleared() {
        app?.close()
        app = null
        super.onCleared()
    }

    companion object {
        private fun messageFor(error: ForgeAppException): String =
            when (error) {
                is ForgeAppException.WouldUnderflow -> "Count cannot go below zero."
                is ForgeAppException.WouldOverflow -> "Count cannot exceed one million."
                is ForgeAppException.PersistenceFailed -> "Could not save counter state."
                is ForgeAppException.CorruptPersistence -> "Stored counter data is invalid."
                is ForgeAppException.UnsupportedSchemaVersion ->
                    "Saved data is from a newer version of this app."
                is ForgeAppException.PersistenceReadFailed -> "Could not read saved data."
                is ForgeAppException.StorageDirectoryUnavailable -> "Storage folder is unavailable."
                is ForgeAppException.InvalidInput -> "That value is not allowed."
                is ForgeAppException.Cancelled -> "The operation was cancelled."
                is ForgeAppException.ShutDown -> "The counter has been closed. Reopen it to continue."
                is ForgeAppException.OperationInProgress -> "Another operation is still running."
                is ForgeAppException.InternalFailure -> "Internal error in Rust core."
            }
    }
}
