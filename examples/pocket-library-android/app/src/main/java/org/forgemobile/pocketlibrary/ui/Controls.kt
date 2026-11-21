// Controls the book form and a book's screen share: the reading status picker and the star
// rating, with the iPhone app's identifiers.
package org.forgemobile.pocketlibrary.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.ui.theme.Forge
import org.forgemobile.pocketlibrary.ui.theme.toColor
import uniffi.pocket_library_core.ReadingStatus

/** A status's identifier part: `<tag>.wantToRead`, as the iPhone app's enum cases. */
internal fun ReadingStatus.key(): String = when (this) {
    ReadingStatus.WANT_TO_READ -> "wantToRead"
    ReadingStatus.READING -> "reading"
    ReadingStatus.FINISHED -> "finished"
}

/**
 * The reading status as a menu button. The button ([tag]) shows the status; each choice is
 * `<tag>.<status>`.
 */
@Composable
internal fun StatusPicker(status: ReadingStatus, tag: String, onSelect: (ReadingStatus) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        OutlinedButton(
            onClick = { open = true },
            modifier = Modifier.testTag(tag),
        ) {
            Text(LibraryModel.statusLabel(status))
            Spacer(Modifier.width(4.dp))
            Icon(Icons.Filled.ArrowDropDown, contentDescription = null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (choice in ReadingStatus.entries) {
                DropdownMenuItem(
                    text = { Text(LibraryModel.statusLabel(choice)) },
                    onClick = {
                        open = false
                        if (choice != status) onSelect(choice)
                    },
                    modifier = Modifier.testTag("$tag.${choice.key()}"),
                )
            }
        }
    }
}

/**
 * 1–5 stars; tapping the current rating clears it. Each star is `<prefix>.star.<n>`, named
 * "n stars" for TalkBack; the row (`<prefix>.rating`) says the rating.
 */
@Composable
internal fun StarRating(rating: UByte?, prefix: String, onChange: (UByte?) -> Unit) {
    val current = rating?.toInt() ?: 0
    Row(
        Modifier
            .fillMaxWidth()
            .testTag("$prefix.rating")
            .semantics { stateDescription = if (current == 0) "Not rated" else "$current of 5 stars" },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(0.dp),
    ) {
        for (star in 1..5) {
            val filled = current >= star
            IconButton(
                onClick = { onChange(if (current == star) null else star.toUByte()) },
                modifier = Modifier
                    .testTag("$prefix.star.$star")
                    .semantics {
                        role = Role.Button
                        selected = filled
                    },
            ) {
                Icon(
                    Icons.Filled.Star,
                    contentDescription = if (star == 1) "1 star" else "$star stars",
                    tint = if (filled) Forge.colors.warning.solid.toColor() else MaterialTheme.colorScheme.outlineVariant,
                )
            }
        }
        Spacer(Modifier.weight(1f))
        if (current == 0) Text("Not rated", style = MaterialTheme.typography.bodySmall)
    }
}
