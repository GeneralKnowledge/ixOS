package org.forgemobile.relay.kit

/** Something a screen loads, and how that went (ForgeLoad in Swift). */
sealed interface ForgeLoad<out V> {
    data object Idle : ForgeLoad<Nothing>
    data object Loading : ForgeLoad<Nothing>
    data class Loaded<V>(override val value: V) : ForgeLoad<V>
    data class Failed(override val failure: ForgeFailure) : ForgeLoad<Nothing>

    val value: V? get() = (this as? Loaded<V>)?.value
    val failure: ForgeFailure? get() = (this as? Failed)?.failure
    val isLoading: Boolean get() = this is Loading

    /** Loading, unless there is already a value to keep showing while it reloads. */
    val whileReloading: ForgeLoad<V> get() = if (value != null) this else Loading

    fun <T> map(transform: (V) -> T): ForgeLoad<T> = when (this) {
        Idle -> Idle
        Loading -> Loading
        is Loaded -> Loaded(transform(value))
        is Failed -> this
    }

    companion object {
        /** Runs [work]: its value, its failure, or [Idle] when it was cancelled. */
        suspend fun <V> capture(work: suspend () -> V): ForgeLoad<V> = try {
            Loaded(work())
        } catch (e: Throwable) {
            if (ForgeFailure.isCancellation(e)) Idle else Failed(ForgeFailure.from(e))
        }
    }

    /** Reloads, keeping the current state when the reload is cancelled. */
    suspend fun reloaded(work: suspend () -> @UnsafeVariance V): ForgeLoad<V> {
        val outcome = capture(work)
        if (outcome is Idle) return if (isLoading) Idle else this
        return outcome
    }
}
