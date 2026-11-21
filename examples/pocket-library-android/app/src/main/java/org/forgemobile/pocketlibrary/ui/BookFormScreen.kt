// The book form, for a new book and for editing one. It only collects text: Rust validates it
// and applies the reading rules when it is saved, as in the iPhone app's BookFormView. Problems
// Rust names by field are shown under that field; anything else is an alert.
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
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
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
import uniffi.pocket_library_core.ReadingStatus

/** The form's contents, before Rust sees them. */
internal data class BookFormText(
    val title: String = "",
    val author: String = "",
    val status: ReadingStatus = ReadingStatus.WANT_TO_READ,
    val currentPage: String = "",
    val totalPages: String = "",
    val rating: UByte? = null,
    val notes: String = "",
    val description: String = "",
    val coverUrl: String = "",
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

    fun input(): BookInput = BookInput(
        title = title,
        author = author,
        description = description,
        status = status,
        currentPage = currentPage.trim().toUIntOrNull() ?: 0u,
        totalPages = totalPages.trim().takeIf { it.isNotEmpty() }?.toUIntOrNull(),
        rating = rating,
        notes = notes,
        coverUrl = coverUrl.trim().takeIf { it.isNotEmpty() },
    )

    val canSave: Boolean get() = title.isNotBlank()

    companion object {
        /** The edit form's starting values (`BookInput(book)` on iOS). */
        fun from(book: Book) = BookFormText(
            title = book.title,
            author = book.author,
            status = book.status,
            currentPage = if (book.currentPage == 0u) "" else book.currentPage.toString(),
            totalPages = book.totalPages?.toString() ?: "",
            rating = book.rating,
            notes = book.notes,
            description = book.description,
            coverUrl = book.coverUrl ?: "",
        )

        /** Keeps what was typed across rotation and process death. */
        val Saver: Saver<BookFormText, Any> = Saver(
            save = {
                arrayListOf(
                    it.title, it.author, it.status.name, it.currentPage, it.totalPages,
                    it.rating?.toInt() ?: 0, it.notes, it.description, it.coverUrl,
                )
            },
            restore = { saved ->
                val v = saved as List<*>
                BookFormText(
                    title = v[0] as String,
                    author = v[1] as String,
                    status = ReadingStatus.valueOf(v[2] as String),
                    currentPage = v[3] as String,
                    totalPages = v[4] as String,
                    rating = (v[5] as Int).takeIf { it > 0 }?.toUByte(),
                    notes = v[6] as String,
                    description = v[7] as String,
                    coverUrl = v[8] as String,
                )
            },
        )
    }
}

/**
 * The form. With [editing] it edits that book ("Edit Book", Save); otherwise it adds one ("New
 * Book", Add). [onSaved] gets the saved book; [onCancel] leaves without saving.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun BookFormScreen(
    model: LibraryModel,
    editing: Book?,
    onSaved: (Book) -> Unit,
    onCancel: () -> Unit,
) {
    var text by rememberSaveable(stateSaver = BookFormText.Saver) {
        mutableStateOf(editing?.let(BookFormText::from) ?: BookFormText())
    }
    // The Save button's work and its failure, and Rust's validation by field (ForgeAppKit).
    val saving = remember { ForgeSubmission() }
    val isSaving by saving.isWorking.collectAsState()
    val failure by saving.failure.collectAsState()
    var errors by remember { mutableStateOf(ForgeFieldErrors()) }
    // Navigation and screen state change on the main thread. The composition's own dispatcher
    // may resume in place after a Rust call (it does in Compose UI tests), on a pool thread.
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }

    fun save() {
        errors = text.localErrors()
        if (!errors.isEmpty) return
        val input = text.input()
        scope.launch {
            var saved: Book? = null
            saving.run {
                try {
                    saved = if (editing != null) model.update(editing.id, input) else model.add(input)
                } catch (e: LibraryException.InvalidFields) {
                    // Shown under the fields, not as an alert.
                    errors = ErrorText.fieldErrors(e)
                }
            }
            saved?.let(onSaved)
        }
    }

    /** Changes the form and clears the problem Rust named for that field. */
    fun change(field: String, update: (BookFormText) -> BookFormText) {
        text = update(text)
        errors = errors.minus(field)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (editing != null) "Edit Book" else "New Book") },
                navigationIcon = {
                    TextButton(onClick = onCancel, modifier = Modifier.testTag("bookForm.cancel")) { Text("Cancel") }
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
                        ) { Text(if (editing != null) "Save" else "Add") }
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
            FormField("Title", text.title, "title", "bookForm.title", errors, words = true) { v -> change("title") { it.copy(title = v) } }
            FormField("Author", text.author, "author", "bookForm.author", errors, words = true) { v -> change("author") { it.copy(author = v) } }

            SectionTitle("Reading")
            StatusPicker(text.status, tag = "bookForm.status") { s -> change("status") { it.copy(status = s) } }
            FormField("Current page", text.currentPage, "current_page", "bookForm.currentPage", errors, number = true, placeholder = "0") { v ->
                change("current_page") { it.copy(currentPage = v) }
            }
            FormField("Pages", text.totalPages, "total_pages", "bookForm.totalPages", errors, number = true, placeholder = "Unknown") { v ->
                change("total_pages") { it.copy(totalPages = v) }
            }

            SectionTitle("Rating")
            StarRating(text.rating, prefix = "bookForm") { r -> change("rating") { it.copy(rating = r) } }
            ForgeFieldMessage(errors["rating"], tag = "bookForm.rating.error")

            SectionTitle("Notes")
            FormField("Notes", text.notes, "notes", "bookForm.notes", errors, lines = 3, placeholder = "Thoughts, quotes, where you got it…") { v ->
                change("notes") { it.copy(notes = v) }
            }

            SectionTitle("Details")
            FormField("Description", text.description, "description", "bookForm.description", errors, lines = 2) { v ->
                change("description") { it.copy(description = v) }
            }
            FormField("Cover image link", text.coverUrl, "cover_url", "bookForm.cover", errors, url = true, placeholder = "https://…") { v ->
                change("cover_url") { it.copy(coverUrl = v) }
            }
            Text(
                "Covers are shown when you're online. Books found in the catalogue or imported from EPUB keep a copy for offline use.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    ForgeFailureAlert(failure, onDismiss = saving::dismissFailure)
}

@Composable
private fun SectionTitle(title: String) {
    Text(title, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
}

/** A text field with Rust's problem for [field] under it. [lines] > 1 makes it multi-line. */
@Composable
private fun FormField(
    label: String,
    value: String,
    field: String,
    tag: String,
    errors: ForgeFieldErrors,
    number: Boolean = false,
    words: Boolean = false,
    url: Boolean = false,
    lines: Int = 1,
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
            singleLine = lines == 1,
            minLines = lines,
            isError = message != null,
            keyboardOptions = when {
                number -> KeyboardOptions(keyboardType = KeyboardType.Number)
                url -> KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false)
                words -> KeyboardOptions(capitalization = KeyboardCapitalization.Words)
                else -> KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
            },
            modifier = Modifier.fillMaxWidth().testTag(tag),
        )
        ForgeFieldMessage(message, tag = "$tag.error")
    }
}
