package org.forgemobile.counter

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import java.io.File
import uniffi.forge_ffi.ForgeActionKind
import uniffi.forge_ffi.ForgeAppException
import uniffi.forge_ffi.ForgeCounterApp
import uniffi.forge_ffi.StartupKind
import uniffi.forge_ffi.forgeGreeting
import uniffi.forge_ffi.recoverCounterApp

class CounterViewModel(context: Context) : ViewModel() {
    var count by mutableStateOf(0L)
        private set
    var startupLabel by mutableStateOf("")
        private set
    var errorMessage by mutableStateOf<String?>(null)
        private set
    var isBlocked by mutableStateOf(false)
        private set
    var canRecoverFromCorruption by mutableStateOf(false)
        private set

    val greeting: String = forgeGreeting()

    private var app: ForgeCounterApp? = null
    private val storageDirectory: String = storageDir(context)

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
        app?.close()
        app = opened
        isBlocked = false
        count = try {
            opened.snapshot().count
        } catch (_: ForgeAppException) {
            0L
        }
        startupLabel =
            when {
                afterRecovery -> "Recovered — fresh counter"
                else ->
                    when (try {
                        opened.startupKind()
                    } catch (_: ForgeAppException) {
                        null
                    }) {
                        StartupKind.FRESH_INSTALL -> "Fresh install"
                        StartupKind.RESTORED -> "Restored saved state"
                        null -> ""
                    }
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
        when (error) {
            is ForgeAppException.CorruptPersistence -> {
                canRecoverFromCorruption = true
                errorMessage =
                    "Stored counter data is invalid. You can reset storage or retry opening."
            }
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
        private fun storageDir(context: Context): String {
            val base = File(context.filesDir, "counter")
            base.mkdirs()
            return base.absolutePath
        }

        private fun messageFor(error: ForgeAppException): String =
            when (error) {
                is ForgeAppException.WouldUnderflow -> "Count cannot go below zero."
                is ForgeAppException.WouldOverflow -> "Count cannot exceed one million."
                is ForgeAppException.PersistenceFailed -> "Could not save counter state."
                is ForgeAppException.CorruptPersistence -> "Stored counter data is invalid."
                is ForgeAppException.PersistenceReadFailed -> "Could not read saved data."
                is ForgeAppException.StorageDirectoryUnavailable -> "Storage folder is unavailable."
                is ForgeAppException.InternalFailure -> "Internal error in Rust core."
            }
    }
}
