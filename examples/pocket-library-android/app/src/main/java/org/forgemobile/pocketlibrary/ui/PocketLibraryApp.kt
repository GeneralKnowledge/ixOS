// Pocket Library's Compose screens on Android: the library list, the New Book form, and a
// book's detail with delete. Material 3 with Navigation Compose; the look comes with plan
// milestone A2. Test tags use the iOS app's accessibility identifiers (`screen.element`) and
// are exposed as resource ids, so UI Automator finds them too.
package org.forgemobile.pocketlibrary.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.kit.ForgeLoadView
import uniffi.pocket_library_core.Book

/** The screens, as typed routes. A route survives rotation and process death. */
@Serializable
private data object LibraryRoute

@Serializable
private data object NewBookRoute

/** Rust's book ids are unsigned; navigation arguments are signed, so the bits are kept. */
@Serializable
private data class BookRoute(val id: Long) {
    val bookId: ULong get() = id.toULong()
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun PocketLibraryApp(model: LibraryModel) {
    val colors = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
    MaterialTheme(colorScheme = colors) {
        Box(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }) {
            val opening by model.opening.collectAsState()
            ForgeLoadView(
                opening,
                onRetry = model::open,
                failureTag = "open.error",
                retryTag = "open.retry",
            ) { Screens(model) }
        }
    }
}

@Composable
private fun Screens(model: LibraryModel) {
    val nav = rememberNavController()
    NavHost(nav, startDestination = LibraryRoute) {
        composable<LibraryRoute> {
            LibraryScreen(
                model,
                onAdd = { nav.navigate(NewBookRoute) { launchSingleTop = true } },
                onOpen = { nav.navigate(BookRoute(it.toLong())) { launchSingleTop = true } },
            )
        }
        composable<NewBookRoute> {
            BookFormScreen(model, onDone = { nav.popBackStack() })
        }
        composable<BookRoute> { entry ->
            BookDetailScreen(model, entry.toRoute<BookRoute>().bookId, onClose = { nav.popBackStack() })
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
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text("No Books Yet", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("library.empty"))
                    Text("Add a book you're reading or want to read.", style = MaterialTheme.typography.bodyMedium)
                    Button(onClick = onAdd, modifier = Modifier.testTag("library.empty.add")) { Text("Add Book") }
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

private enum class DetailAction { DELETE }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BookDetailScreen(model: LibraryModel, id: ULong, onClose: () -> Unit) {
    val books by model.books.collectAsState()
    val book = books.firstOrNull { it.id == id }
    var alert by remember { mutableStateOf<ForgeAlert<DetailAction>?>(null) }
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
            Text(
                LibraryModel.statusLabel(book.status),
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.testTag("bookDetail.status"),
            )
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

@Composable
internal fun Centered(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}
