package org.forgemobile.pocketlibrary.kit

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * A button's work in progress and its failure (ForgeSubmission in Swift): one run at a time,
 * the button shows progress while it works, and a failure (not a cancellation) is kept for an
 * alert.
 */
class ForgeSubmission {
    private val _isWorking = MutableStateFlow(false)
    val isWorking: StateFlow<Boolean> = _isWorking.asStateFlow()

    private val _failure = MutableStateFlow<ForgeFailure?>(null)
    val failure: StateFlow<ForgeFailure?> = _failure.asStateFlow()

    /** Runs [work] unless a run is already going. True when it finished without an error. */
    suspend fun run(work: suspend () -> Unit): Boolean {
        if (!_isWorking.compareAndSet(expect = false, update = true)) return false
        _failure.value = null
        try {
            work()
            return true
        } catch (e: Throwable) {
            if (ForgeFailure.isCancellation(e)) throw e
            _failure.value = ForgeFailure.from(e)
            return false
        } finally {
            _isWorking.value = false
        }
    }

    /** Clears the failure once its alert is dismissed. */
    fun dismissFailure() {
        _failure.value = null
    }
}
