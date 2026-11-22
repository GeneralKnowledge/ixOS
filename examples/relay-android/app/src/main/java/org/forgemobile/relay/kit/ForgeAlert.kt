package org.forgemobile.relay.kit

/**
 * An alert or confirmation as a model value (ForgeAlert in Swift): the model decides what to
 * ask, the screen shows it, and the chosen button's [ForgeAlertButton.action] comes back to the
 * model. Button identifiers become test tags.
 */
data class ForgeAlert<out A>(
    val title: String,
    val message: String? = null,
    val buttons: List<ForgeAlertButton<A>>,
    val style: Style = Style.ALERT,
) {
    /** On Android both show as a dialog; a destructive confirmation colours its button. */
    enum class Style { ALERT, CONFIRMATION }

    fun action(title: String): A? = buttons.firstOrNull { it.title == title }?.action

    companion object {
        fun <A> failure(failure: ForgeFailure): ForgeAlert<A> =
            ForgeAlert(failure.title, failure.message, listOf(ForgeAlertButton.ok()))

        /** The alert for [error], or null for a cancellation. */
        fun <A> failure(error: Throwable): ForgeAlert<A>? =
            if (ForgeFailure.isCancellation(error)) null else failure(ForgeFailure.from(error))

        /** A failure that offers Try Again, sending [retry], when it is retryable. */
        fun <A> failure(failure: ForgeFailure, retry: A): ForgeAlert<A> {
            if (!failure.isRetryable) return failure(failure)
            return ForgeAlert(
                failure.title,
                failure.message,
                listOf(
                    ForgeAlertButton("Try Again", action = retry, identifier = "alert.retry"),
                    ForgeAlertButton("Cancel", role = ForgeAlertButton.Role.CANCEL, identifier = "alert.ok"),
                ),
            )
        }

        fun <A> failure(error: Throwable, retry: A): ForgeAlert<A>? =
            if (ForgeFailure.isCancellation(error)) null else failure(ForgeFailure.from(error), retry)

        /**
         * Asks before doing something. The buttons are `<identifier>.confirm` and
         * `<identifier>.cancel`.
         */
        fun <A> confirm(
            title: String,
            message: String? = null,
            confirm: String,
            action: A,
            identifier: String,
            destructive: Boolean = false,
        ): ForgeAlert<A> = ForgeAlert(
            title,
            message,
            listOf(
                ForgeAlertButton(
                    confirm,
                    role = if (destructive) ForgeAlertButton.Role.DESTRUCTIVE else null,
                    action = action,
                    identifier = "$identifier.confirm",
                ),
                ForgeAlertButton("Cancel", role = ForgeAlertButton.Role.CANCEL, identifier = "$identifier.cancel"),
            ),
            if (destructive) Style.CONFIRMATION else Style.ALERT,
        )
    }
}

data class ForgeAlertButton<out A>(
    val title: String,
    val role: Role? = null,
    /** Sent back when the button is chosen; null just dismisses. */
    val action: A? = null,
    val identifier: String,
) {
    enum class Role { CANCEL, DESTRUCTIVE }

    companion object {
        fun <A> ok(): ForgeAlertButton<A> = ForgeAlertButton("OK", role = Role.CANCEL, identifier = "alert.ok")
    }
}
