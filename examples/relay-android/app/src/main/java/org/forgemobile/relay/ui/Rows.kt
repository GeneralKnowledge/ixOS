// The list pieces Relay's screens are made of: grouped sections with a header and footer,
// switches, values and choices, the Android form of SwiftUI's Form rows on the iPhone.
package org.forgemobile.relay.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
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
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import org.forgemobile.relay.ui.theme.Forge
import org.forgemobile.relay.ui.theme.ForgeTextRole

/** A section's title, above its rows. */
fun LazyListScope.sectionHeader(title: String, key: String) {
    item(key = "header.$key") {
        Text(
            title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 24.dp, bottom = 4.dp),
        )
    }
}

/** What a section means, below its rows. */
fun LazyListScope.sectionFooter(text: String, key: String, tag: String? = null) {
    item(key = "footer.$key") { Footer(text, tag) }
}

@Composable
fun Footer(text: String, tag: String? = null) {
    Text(
        text,
        style = Forge.text(ForgeTextRole.NOTE),
        modifier = Modifier
            .padding(horizontal = 16.dp, vertical = 6.dp)
            .then(if (tag != null) Modifier.testTag(tag) else Modifier),
    )
}

/** A switch with its label; the whole row toggles it and reads as one switch. */
@Composable
fun SwitchRow(label: String, checked: Boolean, tag: String, onChange: (Boolean) -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .testTag(tag)
            .toggleable(value = checked, role = Role.Switch, onValueChange = onChange)
            .padding(horizontal = 16.dp, vertical = 8.dp),
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = null)
    }
}

/** A label and its value, read as one element. */
@Composable
fun ValueRow(label: String, value: String, tag: String, onClick: (() -> Unit)? = null) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .testTag(tag)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .semantics(mergeDescendants = true) {}
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Spacer(Modifier.width(12.dp))
        Text(value, style = Forge.text(ForgeTextRole.METADATA))
    }
}

/** One of a few choices, picked from a menu (SwiftUI's menu Picker). */
@Composable
fun <T> ChoiceRow(
    label: String,
    choice: T,
    choices: List<T>,
    name: (T) -> String,
    tag: String,
    onChoose: (T) -> Unit,
) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .testTag(tag)
                .clickable(role = Role.DropdownList) { open = true }
                .semantics(mergeDescendants = true) { stateDescription = name(choice) }
                .padding(horizontal = 16.dp, vertical = 12.dp),
        ) {
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(name(choice), style = Forge.text(ForgeTextRole.METADATA))
            Icon(Icons.Filled.ArrowDropDown, contentDescription = null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            choices.forEachIndexed { index, option ->
                DropdownMenuItem(
                    text = { Text(name(option)) },
                    onClick = {
                        open = false
                        if (option != choice) onChoose(option)
                    },
                    modifier = Modifier.testTag("$tag.$index"),
                )
            }
        }
    }
}

@Composable
fun RowDivider() {
    HorizontalDivider(Modifier.padding(start = 16.dp))
}

/** Text in a section, for a status or an explanation that isn't a row of its own. */
@Composable
fun SectionText(text: String, tag: String? = null) {
    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
        Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            modifier = if (tag != null) Modifier.testTag(tag) else Modifier,
        )
    }
}
