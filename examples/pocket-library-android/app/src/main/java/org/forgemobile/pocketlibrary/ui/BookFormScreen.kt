// The New Book form. It only collects text: Rust validates it and applies the reading rules
// when it is saved, as in the iPhone app's BookFormView. Problems Rust names by field are shown
// under that field; anything else is an alert.
package org.forgemobile.pocketlibrary.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.ErrorText
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeFailureAlert
import org.forgemobile.pocketlibrary.kit.ForgeFieldErrors
import org.forgemobile.pocketlibrary.kit.ForgeFieldMessage
import org.forgemobile.pocketlibrary.kit.ForgeSubmission
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.BookInput
import uniffi.pocket_library_core.LibraryException

/** The form's text, before Rust sees it. */
internal data class BookFormText(
    val title: String = "",
    val author: String = "",
    val currentPage: String = "",
    val totalPages: String = "",
) {
    /**
     * Numbers that aren't numbers never reach Rust; this says so under their field. Empty is
     * fine: no current page is 0, no page count is unknown.
     */
    fun localErrors(): ForgeFieldErrors {
        var errors = ForgeFieldErrors()
        for ((field, text) in listOf("current_page" to currentPage, "total_pages" to totalPages)) {
            val trimmed = text.trim()
            if (trimmed.isNotEmpty() && trimmed.toUIntOrNull() == null) {
                errors = errors.plus(field, "Enter a whole number.")
            }
        }
        return errors
    }

    fun input(): BookInput = LibraryModel.emptyInput().copy(
        title = title,
        author = author,
        currentPage = currentPage.trim().toUIntOrNull() ?: 0u,
        totalPages = totalPages.trim().takeIf { it.isNotEmpty() }?.toUIntOrNull(),
    )

    val canSave: Boolean get() = title.isNotBlank()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun BookFormScreen(model: LibraryModel, onDone: () -> Unit) {
    var title by rememberSaveable { mutableStateOf("") }
    var author by rememberSaveable { mutableStateOf("") }
    var currentPage by rememberSaveable { mutableStateOf("") }
    var totalPages by rememberSaveable { mutableStateOf("") }
    val text = BookFormText(title, author, currentPage, totalPages)
    // The Add button's work and its failure, and Rust's validation by field (ForgeAppKit).
    val saving = remember { ForgeSubmission() }
    val isSaving by saving.isWorking.collectAsState()
    val failure by saving.failure.collectAsState()
    var errors by remember { mutableStateOf(ForgeFieldErrors()) }
    val scope = rememberCoroutineScope()

    fun save() {
        errors = text.localErrors()
        if (!errors.isEmpty) return
        val input = text.input()
        scope.launch {
            var saved: Book? = null
            saving.run {
                try {
                    saved = model.add(input)
                } catch (e: LibraryException.InvalidFields) {
                    // Shown under the fields, not as an alert.
                    errors = ErrorText.fieldErrors(e)
                }
            }
            if (saved != null) onDone()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("New Book") },
                navigationIcon = {
                    TextButton(onClick = onDone, modifier = Modifier.testTag("bookForm.cancel")) { Text("Cancel") }
                },
                actions = {
                    if (isSaving) {
                        CircularProgressIndicator(
                            Modifier.padding(horizontal = 16.dp).size(24.dp).semantics { contentDescription = "Saving" },
                        )
                    } else {
                        TextButton(
                            onClick = ::save,
                            enabled = text.canSave,
                            modifier = Modifier.testTag("bookForm.save"),
                        ) { Text("Add") }
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp)
                .fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            FormField("Title", title, "title", "bookForm.title", errors) { title = it; errors = errors.minus("title") }
            FormField("Author", author, "author", "bookForm.author", errors) { author = it; errors = errors.minus("author") }
            Text("Reading", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
            FormField("Current page", currentPage, "current_page", "bookForm.currentPage", errors, number = true) {
                currentPage = it
                errors = errors.minus("current_page")
            }
            FormField("Pages", totalPages, "total_pages", "bookForm.totalPages", errors, number = true, placeholder = "Unknown") {
                totalPages = it
                errors = errors.minus("total_pages")
            }
        }
    }
    ForgeFailureAlert(failure, onDismiss = saving::dismissFailure)
}

/** A text field with Rust's problem for [field] under it. */
@Composable
private fun FormField(
    label: String,
    value: String,
    field: String,
    tag: String,
    errors: ForgeFieldErrors,
    number: Boolean = false,
    placeholder: String? = null,
    onChange: (String) -> Unit,
) {
    val message = errors[field]
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        OutlinedTextField(
            value = value,
            onValueChange = onChange,
            label = { Text(label) },
            placeholder = placeholder?.let { { Text(it) } },
            singleLine = true,
            isError = message != null,
            keyboardOptions = if (number) {
                KeyboardOptions(keyboardType = KeyboardType.Number)
            } else {
                KeyboardOptions(capitalization = KeyboardCapitalization.Words)
            },
            modifier = Modifier.fillMaxWidth().testTag(tag),
        )
        ForgeFieldMessage(message, tag = "$tag.error")
    }
}
