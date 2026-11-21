// Compose views for the ForgeAppKit counterparts (forgekit/Sources/ForgeAppKit/Views). Model
// values go in, choices come back as callbacks; identifiers become test tags.
package org.forgemobile.pocketlibrary.kit

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/**
 * Shows [alert] while it is not null. A button with an action sends it to [onAction]; every
 * button, and tapping outside, then calls [onDismiss], so the model clears its alert.
 */
@Composable
fun <A> ForgeAlertDialog(alert: ForgeAlert<A>?, onDismiss: () -> Unit, onAction: (A) -> Unit = {}) {
    if (alert == null) return
    val confirm = alert.buttons.firstOrNull { it.role != ForgeAlertButton.Role.CANCEL } ?: alert.buttons.first()
    val dismiss = alert.buttons.firstOrNull { it !== confirm }
    @Composable
    fun ButtonFor(button: ForgeAlertButton<A>) {
        TextButton(
            onClick = {
                onDismiss()
                button.action?.let(onAction)
            },
            modifier = Modifier.testTag(button.identifier),
        ) {
            val colour = if (button.role == ForgeAlertButton.Role.DESTRUCTIVE) {
                MaterialTheme.colorScheme.error
            } else {
                MaterialTheme.colorScheme.primary
            }
            Text(button.title, color = colour)
        }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(alert.title) },
        text = alert.message?.let { message -> { Text(message) } },
        confirmButton = { ButtonFor(confirm) },
        dismissButton = dismiss?.let { button -> { ButtonFor(button) } },
    )
}

/** A failure as an OK alert (`.forgeAlert(failure:)` in Swift). */
@Composable
fun ForgeFailureAlert(failure: ForgeFailure?, onDismiss: () -> Unit) {
    ForgeAlertDialog<Nothing>(failure?.let { ForgeAlert.failure(it) }, onDismiss)
}

/**
 * A field's problem, under the field. TalkBack reads it as "Error: …", as VoiceOver does on
 * iOS.
 */
@Composable
fun ForgeFieldMessage(message: String?, tag: String) {
    if (message == null) return
    Text(
        message,
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodySmall,
        modifier = Modifier
            .testTag(tag)
            .semantics { contentDescription = "Error: $message" },
    )
}

/**
 * The content for [load]: a spinner while loading, the failure with Try Again (when it is
 * retryable) if it failed, and [content] once there is a value.
 */
@Composable
fun <V> ForgeLoadView(
    load: ForgeLoad<V>,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    /** Test tag of the failure's message (its title is `<failureTag>.title`). */
    failureTag: String = "load.failure",
    /** Test tag of Try Again, which the caller names, as on iOS (`open.retry`). */
    retryTag: String = "load.retry",
    /** More ways out of a failure, below Try Again (Pocket Library's Start Fresh). */
    failureActions: @Composable () -> Unit = {},
    content: @Composable (V) -> Unit,
) {
    when (load) {
        is ForgeLoad.Loaded -> content(load.value)
        is ForgeLoad.Failed -> Box(modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    load.failure.title,
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.testTag("$failureTag.title"),
                )
                Text(
                    load.failure.message,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.testTag(failureTag),
                )
                if (load.failure.isRetryable) {
                    Button(onClick = onRetry, modifier = Modifier.testTag(retryTag)) { Text("Try Again") }
                }
                failureActions()
            }
        }
        ForgeLoad.Idle, ForgeLoad.Loading -> Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(Modifier.testTag("load.progress"))
        }
    }
}
