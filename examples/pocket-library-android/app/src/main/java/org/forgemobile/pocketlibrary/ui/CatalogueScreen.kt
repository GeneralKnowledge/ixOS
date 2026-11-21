// Find a Book: searches the online catalogue (Open Library) and adds a result, as the iPhone
// app's CatalogueSearchView. Every failure leaves the library as it was and offers the manual
// form instead. Leaving the screen, or searching again, cancels the search, and Rust stops its
// request.
package org.forgemobile.pocketlibrary.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AddCircle
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.kit.ForgeLoad
import org.forgemobile.pocketlibrary.ui.theme.Forge
import org.forgemobile.pocketlibrary.ui.theme.ForgeTextRole
import org.forgemobile.pocketlibrary.ui.theme.toColor
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.CatalogueBook

/** A search to run: [attempt] makes Try Again, or the same words again, a new search. */
internal data class CatalogueSearchRequest(val text: String, val attempt: Int) {
    val retried: CatalogueSearchRequest get() = copy(attempt = attempt + 1)

    companion object {
        /** The search for what was typed, or null when there is nothing to find. */
        fun submitted(query: String, after: CatalogueSearchRequest?): CatalogueSearchRequest? {
            val text = query.trim()
            if (text.isEmpty()) return null
            return CatalogueSearchRequest(text, (after?.attempt ?: 0) + 1)
        }
    }
}

/**
 * [onOpen] opens a book (added, or already in the library); [onManual] opens the form instead;
 * [onCancel] leaves, which stops a search in progress.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun CatalogueScreen(
    model: LibraryModel,
    onOpen: (Book) -> Unit,
    onManual: () -> Unit,
    onCancel: () -> Unit,
) {
    var query by rememberSaveable { mutableStateOf("") }
    var submitted by remember { mutableStateOf<CatalogueSearchRequest?>(null) }
    var state by remember { mutableStateOf<ForgeLoad<List<CatalogueBook>>>(ForgeLoad.Idle) }
    var importingKey by remember { mutableStateOf<String?>(null) }
    var notes by remember { mutableStateOf<Pair<Book, List<String>>?>(null) }
    var alert by remember { mutableStateOf<ForgeAlert<Nothing>?>(null) }
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    val focus = LocalFocusManager.current
    val searchFocus = remember { FocusRequester() }

    // The search for the submitted words. A new request, or leaving the screen, cancels it;
    // the model then stops the Rust search and its request.
    LaunchedEffect(submitted) {
        val request = submitted ?: return@LaunchedEffect
        state = ForgeLoad.Loading
        val outcome = ForgeLoad.capture { model.searchCatalogue(request.text) }
        // A stopped search shows nothing new; its replacement sets the screen.
        if (outcome !is ForgeLoad.Idle) state = outcome
    }
    // Starts with the keyboard up, as on the iPhone.
    LaunchedEffect(Unit) { searchFocus.requestFocus() }

    fun submit() {
        val request = CatalogueSearchRequest.submitted(query, after = submitted) ?: return
        focus.clearFocus()
        submitted = request
    }

    fun add(result: CatalogueBook) {
        result.libraryBookId?.let { id -> model.book(id)?.let { onOpen(it); return } }
        importingKey = result.key
        scope.launch {
            try {
                val imported = model.importFromCatalogue(result.key)
                if (imported.warnings.isEmpty()) onOpen(imported.book) else notes = imported.book to imported.warnings
            } catch (e: Exception) {
                alert = ForgeAlert.failure(e)
            } finally {
                importingKey = null
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Find a Book") },
                navigationIcon = {
                    TextButton(onClick = onCancel, modifier = Modifier.testTag("catalogue.cancel")) { Text("Cancel") }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                placeholder = { Text("Title, author or ISBN") },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { submit() }),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = Forge.spacing.l.dp, vertical = Forge.spacing.s.dp)
                    .focusRequester(searchFocus)
                    .testTag("catalogue.search"),
            )
            when (val current = state) {
                ForgeLoad.Idle -> Notice(
                    "Search Open Library",
                    "Find a book by title or author and add it with its cover and description.",
                    tag = "catalogue.idle",
                )
                ForgeLoad.Loading -> Centered {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        CircularProgressIndicator(Modifier.testTag("catalogue.searching"))
                        Text("Searching…", style = Forge.text(ForgeTextRole.METADATA))
                    }
                }
                is ForgeLoad.Failed -> Notice(current.failure.title, current.failure.message, tag = "catalogue.error") {
                    if (current.failure.isRetryable) {
                        Button(
                            onClick = { submitted = submitted?.retried },
                            modifier = Modifier.testTag("catalogue.retry"),
                        ) { Text("Try Again") }
                    }
                    OutlinedButton(onClick = onManual, modifier = Modifier.testTag("catalogue.manual")) {
                        Text("Enter Details by Hand")
                    }
                }
                is ForgeLoad.Loaded -> if (current.value.isEmpty()) {
                    Notice(
                        "No Results for “${submitted?.text ?: query}”",
                        "Try other words, or enter the book's details yourself.",
                        tag = "catalogue.noResults",
                    ) {
                        OutlinedButton(onClick = onManual, modifier = Modifier.testTag("catalogue.noResults.manual")) {
                            Text("Enter Details by Hand")
                        }
                    }
                } else {
                    LazyColumn(Modifier.fillMaxSize().testTag("catalogue.results")) {
                        items(current.value, key = { it.key }) { result ->
                            CatalogueRow(
                                result,
                                importing = importingKey == result.key,
                                enabled = result.libraryBookId != null || importingKey == null,
                            ) { add(result) }
                            HorizontalDivider()
                        }
                    }
                }
            }
        }
    }
    ForgeAlertDialog(alert, onDismiss = { alert = null }) {}
    notes?.let { (book, warnings) ->
        AlertDialog(
            onDismissRequest = { notes = null; onOpen(book) },
            title = { Text("Added Without Some Details") },
            text = { Text(warnings.joinToString("\n")) },
            confirmButton = {
                TextButton(onClick = { notes = null; onOpen(book) }, modifier = Modifier.testTag("catalogue.notes.ok")) { Text("OK") }
            },
        )
    }
}

/** A result: the whole row adds it, or opens it once it is in the library. */
@Composable
private fun CatalogueRow(result: CatalogueBook, importing: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val inLibrary = result.libraryBookId != null
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, onClick = onClick)
            .semantics(mergeDescendants = true) {
                contentDescription = if (inLibrary) "${result.title}, already in your library. Open it" else "Add ${result.title}"
            }
            .testTag(if (inLibrary) "catalogue.open" else "catalogue.add")
            .padding(horizontal = Forge.spacing.l.dp, vertical = Forge.spacing.m.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(result.title, style = Forge.text(ForgeTextRole.ROW_TITLE), maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (result.author.isNotEmpty()) {
                Text(result.author, style = Forge.text(ForgeTextRole.METADATA), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            val details = listOfNotNull(result.firstPublished?.toString(), result.totalPages?.let { "$it pages" })
            if (details.isNotEmpty()) {
                Text(details.joinToString(" · "), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Spacer(Modifier.width(8.dp))
        when {
            importing -> CircularProgressIndicator(Modifier.size(24.dp))
            inLibrary -> Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = Forge.colors.success.solid.toColor())
            else -> Icon(Icons.Filled.AddCircle, contentDescription = null, tint = Forge.colors.accent.solid.toColor())
        }
    }
}

@Composable
private fun Notice(title: String, detail: String, tag: String, actions: @Composable () -> Unit = {}) {
    Centered(Modifier.padding(24.dp)) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, style = Forge.text(ForgeTextRole.SECTION_TITLE), textAlign = TextAlign.Center, modifier = Modifier.testTag(tag))
            Text(detail, style = Forge.text(ForgeTextRole.METADATA), textAlign = TextAlign.Center)
            actions()
        }
    }
}
