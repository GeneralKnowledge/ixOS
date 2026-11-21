// One book: progress, status, rating, notes and description; edit, share and delete. Every
// change goes to Rust, which applies the reading rules (a page on an unread book starts it, the
// last page finishes it), as in the iPhone app's BookDetailView.
package org.forgemobile.pocketlibrary.ui

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.ui.theme.Forge
import org.forgemobile.pocketlibrary.ui.theme.ForgeTextRole
import org.forgemobile.pocketlibrary.ui.theme.toColor
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.BookSource
import uniffi.pocket_library_core.ReadingStatus

private enum class DetailAction { DELETE }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun BookDetailScreen(model: LibraryModel, id: ULong, onEdit: () -> Unit, onRead: () -> Unit, onClose: () -> Unit) {
    // Every book, not the filtered list: a change here may take the book out of the list.
    val books by model.all.collectAsState()
    val book = books.firstOrNull { it.id == id }
    var alert by remember { mutableStateOf<ForgeAlert<DetailAction>?>(null) }
    // Navigation and screen state change on the main thread. The composition's own dispatcher
    // may resume in place after a Rust call (it does in Compose UI tests), on a pool thread.
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    val context = LocalContext.current

    /** Runs a change and shows its error here. */
    fun run(command: suspend () -> Unit) {
        scope.launch {
            try {
                command()
            } catch (e: Exception) {
                alert = ForgeAlert.failure(e)
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(book?.title ?: "") },
                navigationIcon = {
                    IconButton(onClick = onClose, modifier = Modifier.testTag("bookDetail.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Library")
                    }
                },
                actions = {
                    if (book != null) {
                        IconButton(
                            onClick = {
                                val send = Intent(Intent.ACTION_SEND)
                                    .setType("text/plain")
                                    .putExtra(Intent.EXTRA_TEXT, LibraryModel.shareText(book))
                                context.startActivity(Intent.createChooser(send, null))
                            },
                            modifier = Modifier.testTag("bookDetail.share"),
                        ) { Icon(Icons.Filled.Share, contentDescription = "Share") }
                        TextButton(onClick = onEdit, modifier = Modifier.testTag("bookDetail.edit")) { Text("Edit") }
                    }
                },
            )
        },
    ) { padding ->
        if (book == null) {
            Centered(Modifier.padding(padding)) { Text("Book Deleted") }
            return@Scaffold
        }
        Column(
            Modifier
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(Forge.spacing.l.dp)
                .fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(Forge.spacing.m.dp),
        ) {
            Header(book)
            ReadSection(book, onRead)

            Section("Progress")
            Progress(book, onPage = { page -> run { model.updateProgress(book.id, page) } }, onInvalid = {
                alert = ForgeAlert.failure(ForgeFailure("Check the Details", "Enter a page number."))
            })
            if (book.status != ReadingStatus.FINISHED) {
                FilledTonalButton(
                    onClick = { run { model.setStatus(book.id, ReadingStatus.FINISHED) } },
                    modifier = Modifier.testTag("bookDetail.markFinished"),
                ) {
                    Icon(Icons.Filled.CheckCircle, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Mark as Finished")
                }
            }

            Section("Status")
            StatusPicker(book.status, tag = "bookDetail.status") { status -> run { model.setStatus(book.id, status) } }
            Dates(book)

            Section("Rating")
            StarRating(book.rating, prefix = "bookDetail") { stars ->
                run { model.update(book.id, BookFormText.from(book).copy(rating = stars).input()) }
            }

            if (book.description.isNotEmpty()) {
                Section("Description")
                Text(book.description, style = MaterialTheme.typography.bodyMedium)
            }

            Section("Notes")
            if (book.notes.isEmpty()) {
                TextButton(onClick = onEdit, modifier = Modifier.testTag("bookDetail.addNotes")) { Text("Add notes") }
            } else {
                Text(book.notes, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.testTag("bookDetail.notes"))
            }

            HorizontalDivider()
            OutlinedButton(
                onClick = {
                    alert = ForgeAlert.confirm(
                        "Delete “${book.title}”?",
                        message = "Your notes and progress for this book are deleted. Its reading history stays.",
                        confirm = "Delete Book",
                        action = DetailAction.DELETE,
                        identifier = "bookDetail.delete",
                        destructive = true,
                    )
                },
                modifier = Modifier.fillMaxWidth().testTag("bookDetail.delete"),
            ) { Text("Delete Book", color = MaterialTheme.colorScheme.error) }
        }
    }
    ForgeAlertDialog(alert, onDismiss = { alert = null }) { action ->
        when (action) {
            DetailAction.DELETE -> scope.launch {
                try {
                    model.delete(id)
                    onClose()
                } catch (e: Exception) {
                    alert = ForgeAlert.failure(e)
                }
            }
        }
    }
}

/**
 * Read or Continue Reading for a book with a saved EPUB; for an imported book from before EPUBs
 * were kept, how to make it readable.
 */
@Composable
private fun ReadSection(book: Book, onRead: () -> Unit) {
    if (book.canRead) {
        Button(onClick = onRead, modifier = Modifier.fillMaxWidth().testTag("bookDetail.read")) {
            Text(if (book.readingPosition == null) "Read" else "Continue Reading")
        }
    } else if (book.source is BookSource.Epub) {
        Text(
            "Import this book's EPUB again to read it here. Your progress is kept.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.testTag("bookDetail.reimport"),
        )
    }
}

@Composable
private fun Header(book: Book) {
    Column(verticalArrangement = Arrangement.spacedBy(Forge.spacing.xs.dp)) {
        Text(book.title, style = Forge.text(ForgeTextRole.SCREEN_TITLE), modifier = Modifier.testTag("bookDetail.title"))
        Text(book.author.ifEmpty { "Unknown author" }, style = Forge.text(ForgeTextRole.METADATA))
        StatusBadge(LibraryModel.statusLabel(book.status), Modifier.testTag("bookDetail.badge"))
        when (val source = book.source) {
            is BookSource.Epub -> Text(source.fileName, style = MaterialTheme.typography.labelMedium)
            is BookSource.OpenLibrary -> Text("Open Library", style = MaterialTheme.typography.labelMedium)
            else -> Unit
        }
    }
}

/**
 * The bar, the page field and a stepper. The field saves on the keyboard's Done and when it
 * loses focus; the number keyboard has no Return.
 */
@Composable
private fun Progress(book: Book, onPage: (UInt) -> Unit, onInvalid: () -> Unit) {
    val total = book.totalPages
    if (total != null) {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            LinearProgressIndicator(
                progress = { book.progress.toFloat() },
                color = Forge.colors.accent.solid.toColor(),
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                "${Math.round(book.progress * 100)}% · page ${book.currentPage} of $total",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.testTag("bookDetail.progress"),
            )
        }
    }
    // Starts again from Rust's page whenever it changes (a save here, or anywhere else).
    var pageText by remember(book.currentPage) { mutableStateOf(book.currentPage.toString()) }
    var focused by remember { mutableStateOf(false) }
    val focus = LocalFocusManager.current

    fun save() {
        val page = pageText.trim().toUIntOrNull()
        when {
            page == null -> {
                pageText = book.currentPage.toString()
                onInvalid()
            }
            page != book.currentPage -> onPage(page)
        }
    }

    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
            value = pageText,
            onValueChange = { pageText = it },
            label = { Text("Current page") },
            singleLine = true,
            textStyle = MaterialTheme.typography.bodyLarge.copy(textAlign = TextAlign.End),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Done),
            // Leaving the field saves; Done leaves it.
            keyboardActions = KeyboardActions(onDone = { if (focused) focus.clearFocus() else save() }),
            modifier = Modifier
                .weight(1f)
                .testTag("bookDetail.page")
                .onFocusChanged { state ->
                    if (focused && !state.isFocused) save()
                    focused = state.isFocused
                },
        )
        // The stepper: one page back or on.
        Row(Modifier.testTag("bookDetail.pageStepper")) {
            IconButton(
                onClick = { if (book.currentPage > 0u) onPage(book.currentPage - 1u) },
                enabled = book.currentPage > 0u,
                modifier = Modifier.testTag("bookDetail.pageStepper.decrement").semantics { contentDescription = "Previous page" },
            ) { Text("−", style = MaterialTheme.typography.titleLarge) }
            IconButton(
                onClick = { onPage(book.currentPage + 1u) },
                enabled = total == null || book.currentPage < total,
                modifier = Modifier.testTag("bookDetail.pageStepper.increment").semantics { contentDescription = "Next page" },
            ) { Text("+", style = MaterialTheme.typography.titleLarge) }
        }
    }
}

@Composable
private fun Dates(book: Book) {
    val format = remember { DateFormat.getDateInstance(DateFormat.MEDIUM) }
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        DateRow("Added", format.format(Date(book.addedAtMs)))
        book.startedAtMs?.let { DateRow("Started", format.format(Date(it))) }
        book.finishedAtMs?.let { DateRow("Finished", format.format(Date(it))) }
    }
}

@Composable
private fun DateRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth()) {
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun Section(title: String) {
    Text(title, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = Forge.spacing.s.dp))
}
