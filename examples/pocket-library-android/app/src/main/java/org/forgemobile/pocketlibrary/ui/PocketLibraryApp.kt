// Pocket Library's Compose screens for stage 0 of the Android plan: the library list, adding a
// book, and a book's detail with delete. Plain Material 3; ForgeUI's Compose counterpart comes
// in a later stage. Test tags use the iOS app's accessibility identifiers (`screen.element`)
// and are exposed as resource ids, so UI Automator finds them too.
package org.forgemobile.pocketlibrary.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.host.ForgeHost
import uniffi.pocket_library_core.Book

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun PocketLibraryApp(model: LibraryModel) {
    val colors = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
    MaterialTheme(colorScheme = colors) {
        Box(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }) {
            val status by model.status.collectAsState()
            when (val s = status) {
                ForgeHost.Status.Ready -> Screens(model)
                is ForgeHost.Status.Failed -> Centered { Text(s.message, Modifier.testTag("open.error")) }
                else -> Centered { CircularProgressIndicator() }
            }
        }
    }
}

/** Which screen is showing. Saved as a string so it survives rotation. */
private sealed interface Route {
    data object Library : Route
    data object AddBook : Route
    data class Detail(val id: ULong) : Route

    fun save(): String = when (this) {
        Library -> "library"
        AddBook -> "add"
        is Detail -> "book:$id"
    }

    companion object {
        fun restore(saved: String): Route = when {
            saved == "add" -> AddBook
            saved.startsWith("book:") -> Detail(saved.removePrefix("book:").toULong())
            else -> Library
        }
    }
}

@Composable
private fun Screens(model: LibraryModel) {
    var saved by rememberSaveable { mutableStateOf("library") }
    val route = Route.restore(saved)
    fun go(to: Route) {
        saved = to.save()
    }
    when (route) {
        Route.Library -> LibraryScreen(model, onAdd = { go(Route.AddBook) }, onOpen = { go(Route.Detail(it)) })
        Route.AddBook -> {
            BackHandler { go(Route.Library) }
            BookFormScreen(model, onDone = { go(Route.Library) })
        }
        is Route.Detail -> {
            BackHandler { go(Route.Library) }
            BookDetailScreen(model, route.id, onClose = { go(Route.Library) })
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LibraryScreen(model: LibraryModel, onAdd: () -> Unit, onOpen: (ULong) -> Unit) {
    val books by model.books.collectAsState()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Library") },
                actions = {
                    IconButton(onClick = onAdd, modifier = Modifier.testTag("library.add.manual")) {
                        Icon(Icons.Filled.Add, contentDescription = "Add Book")
                    }
                },
            )
        },
    ) { padding ->
        if (books.isEmpty()) {
            Centered(Modifier.padding(padding)) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("No Books Yet", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("library.empty"))
                    Text("Add a book you're reading or want to read.", style = MaterialTheme.typography.bodyMedium)
                }
            }
        } else {
            LazyColumn(Modifier.padding(padding).fillMaxSize()) {
                items(books, key = { it.id.toLong() }) { book ->
                    BookRow(book, onClick = { onOpen(book.id) })
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
private fun BookRow(book: Book, onClick: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(onClickLabel = "Open ${book.title}", onClick = onClick)
            .testTag("library.book.${book.id}")
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(book.title, style = MaterialTheme.typography.titleMedium)
        if (book.author.isNotBlank()) {
            Text(book.author, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BookFormScreen(model: LibraryModel, onDone: () -> Unit) {
    var title by rememberSaveable { mutableStateOf("") }
    var author by rememberSaveable { mutableStateOf("") }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    var saving by rememberSaveable { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("New Book") },
                navigationIcon = {
                    TextButton(onClick = onDone, modifier = Modifier.testTag("bookForm.cancel")) { Text("Cancel") }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.padding(padding).padding(16.dp).fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                label = { Text("Title") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("bookForm.title"),
            )
            OutlinedTextField(
                value = author,
                onValueChange = { author = it },
                label = { Text("Author") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().testTag("bookForm.author"),
            )
            error?.let {
                Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("bookForm.error"))
            }
            Button(
                onClick = {
                    saving = true
                    scope.launch {
                        try {
                            model.add(title, author)
                            onDone()
                        } catch (e: Exception) {
                            error = LibraryModel.describe(e)
                        } finally {
                            saving = false
                        }
                    }
                },
                enabled = title.isNotBlank() && !saving,
                modifier = Modifier.fillMaxWidth().testTag("bookForm.save"),
            ) { Text("Add Book") }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BookDetailScreen(model: LibraryModel, id: ULong, onClose: () -> Unit) {
    val books by model.books.collectAsState()
    val book = books.firstOrNull { it.id == id }
    var confirming by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(book?.title ?: "Book") },
                navigationIcon = {
                    IconButton(onClick = onClose, modifier = Modifier.testTag("bookDetail.back")) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Library")
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
            Modifier.padding(padding).padding(16.dp).fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(book.title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.testTag("bookDetail.title"))
            if (book.author.isNotBlank()) Text(book.author, style = MaterialTheme.typography.titleMedium)
            Row {
                Text(
                    LibraryModel.statusLabel(book.status),
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.testTag("bookDetail.status"),
                )
            }
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            OutlinedButton(
                onClick = { confirming = true },
                modifier = Modifier.fillMaxWidth().testTag("bookDetail.delete"),
            ) { Text("Delete Book", color = MaterialTheme.colorScheme.error) }
        }
        if (confirming) {
            AlertDialog(
                onDismissRequest = { confirming = false },
                title = { Text("Delete “${book.title}”?") },
                text = { Text("Your notes and progress for this book are deleted. Its reading history stays.") },
                confirmButton = {
                    TextButton(
                        onClick = {
                            confirming = false
                            scope.launch {
                                try {
                                    model.delete(id)
                                    onClose()
                                } catch (e: Exception) {
                                    error = LibraryModel.describe(e)
                                }
                            }
                        },
                        modifier = Modifier.testTag("bookDetail.delete.confirm"),
                    ) { Text("Delete Book", color = MaterialTheme.colorScheme.error) }
                },
                dismissButton = {
                    TextButton(onClick = { confirming = false }, modifier = Modifier.testTag("bookDetail.delete.cancel")) {
                        Text("Cancel")
                    }
                },
            )
        }
    }
}

@Composable
private fun Centered(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}
