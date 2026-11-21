// Pocket Library's Compose screens on Android: the library list with search, filter and sort,
// the book form, and a book's screen (BookDetailScreen.kt). Material 3 with Navigation Compose
// and ForgeUI's tokens. Test tags use the iOS app's accessibility identifiers (`screen.element`)
// and are exposed as resource ids, so UI Automator finds them too.
package org.forgemobile.pocketlibrary.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import kotlinx.serialization.Serializable
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.kit.ForgeLoadView
import org.forgemobile.pocketlibrary.ui.theme.Forge
import org.forgemobile.pocketlibrary.ui.theme.ForgeAccent
import org.forgemobile.pocketlibrary.ui.theme.ForgeTextRole
import org.forgemobile.pocketlibrary.ui.theme.ForgeTheme
import org.forgemobile.pocketlibrary.ui.theme.toColor
import uniffi.pocket_library_core.Book
import uniffi.pocket_library_core.LibrarySort
import uniffi.pocket_library_core.ReadingStatus

/** The screens, as typed routes. A route survives rotation and process death. */
@Serializable
private data object LibraryRoute

@Serializable
private data object NewBookRoute

@Serializable
private data object CatalogueRoute

@Serializable
private data object SettingsRoute

/** Rust's book ids are unsigned; navigation arguments are signed, so the bits are kept. */
@Serializable
private data class BookRoute(val id: Long) {
    val bookId: ULong get() = id.toULong()
}

@Serializable
private data class EditBookRoute(val id: Long) {
    val bookId: ULong get() = id.toULong()
}

/** A book's EPUB in the reader, which covers the app as on the iPhone. */
@Serializable
private data class ReaderRoute(val id: Long) {
    val bookId: ULong get() = id.toULong()
}

/**
 * The app. [dark] forces an appearance (screenshot tests); by default it follows the system.
 */
@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun PocketLibraryApp(model: LibraryModel, dark: Boolean = isSystemInDarkTheme()) {
    // ForgeUI's palette with the iPhone app's accent: ForgeTheme(accent: .orange).
    ForgeTheme(accent = ForgeAccent.ORANGE, dark = dark) {
        Box(
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .semantics { testTagsAsResourceId = true },
        ) {
            val opening by model.opening.collectAsState()
            val canStartFresh by model.canStartFresh.collectAsState()
            ForgeLoadView(
                opening,
                onRetry = model::open,
                failureTag = "open.error",
                retryTag = "open.retry",
                failureActions = { if (canStartFresh) StartFreshButton(model) },
            ) { Screens(model) }
        }
    }
}

private enum class OpenAction { START_FRESH }

/**
 * The way out of an unreadable library, never taken without the person saying so; even then the
 * old file is kept as a backup.
 */
@Composable
private fun StartFreshButton(model: LibraryModel) {
    var alert by remember { mutableStateOf<ForgeAlert<OpenAction>?>(null) }
    TextButton(
        onClick = {
            alert = ForgeAlert.confirm(
                "Start a new, empty library?",
                message = "The unreadable library is kept as a backup file in the app's storage.",
                confirm = "Start New Library",
                action = OpenAction.START_FRESH,
                identifier = "open.startFresh",
                destructive = true,
            )
        },
        modifier = Modifier.testTag("open.startFresh"),
    ) { Text("Start a New Library…") }
    ForgeAlertDialog(alert, onDismiss = { alert = null }) { model.startFresh() }
}

@Composable
private fun Screens(model: LibraryModel) {
    val nav = rememberNavController()
    NavHost(nav, startDestination = LibraryRoute) {
        composable<LibraryRoute> {
            LibraryScreen(
                model,
                onAdd = { nav.navigate(NewBookRoute) { launchSingleTop = true } },
                onFind = { nav.navigate(CatalogueRoute) { launchSingleTop = true } },
                onSettings = { nav.navigate(SettingsRoute) { launchSingleTop = true } },
                onOpen = { nav.navigate(BookRoute(it.toLong())) { launchSingleTop = true } },
            )
        }
        composable<NewBookRoute> {
            BookFormScreen(
                model,
                editing = null,
                // Adding a book opens it, as on the iPhone; Back then returns to the library.
                onSaved = { book ->
                    nav.navigate(BookRoute(book.id.toLong())) { popUpTo<NewBookRoute> { inclusive = true } }
                },
                onCancel = { nav.popBackStack() },
            )
        }
        composable<CatalogueRoute> {
            CatalogueScreen(
                model,
                // An added book opens in place of the search, as on the iPhone.
                onOpen = { book ->
                    nav.navigate(BookRoute(book.id.toLong())) { popUpTo<CatalogueRoute> { inclusive = true } }
                },
                onManual = { nav.navigate(NewBookRoute) { popUpTo<CatalogueRoute> { inclusive = true } } },
                onCancel = { nav.popBackStack() },
            )
        }
        composable<SettingsRoute> {
            SettingsScreen(model, onClose = { nav.popBackStack() })
        }
        composable<EditBookRoute> { entry ->
            val id = entry.toRoute<EditBookRoute>().bookId
            // The book as it was when the form opened; the form keeps what is typed from then on.
            val book = remember(id) { model.book(id) }
            if (book == null) {
                Centered { Text("Book Deleted") }
            } else {
                BookFormScreen(model, editing = book, onSaved = { nav.popBackStack() }, onCancel = { nav.popBackStack() })
            }
        }
        composable<BookRoute> { entry ->
            val id = entry.toRoute<BookRoute>().bookId
            BookDetailScreen(
                model,
                id,
                onEdit = { nav.navigate(EditBookRoute(id.toLong())) { launchSingleTop = true } },
                onRead = { nav.navigate(ReaderRoute(id.toLong())) { launchSingleTop = true } },
                onClose = { nav.popBackStack() },
            )
        }
        composable<ReaderRoute> { entry ->
            ReaderScreen(model, entry.toRoute<ReaderRoute>().bookId, onClose = { nav.popBackStack() })
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LibraryScreen(
    model: LibraryModel,
    onAdd: () -> Unit,
    onFind: () -> Unit,
    onSettings: () -> Unit,
    onOpen: (ULong) -> Unit,
) {
    val all by model.all.collectAsState()
    val books by model.books.collectAsState()
    val search by model.searchText.collectAsState()
    val filter by model.statusFilter.collectAsState()
    var alert by remember { mutableStateOf<ForgeAlert<Nothing>?>(null) }
    var importing by remember { mutableStateOf(false) }
    // An imported book opens, as on the iPhone.
    val onImport = rememberEpubImport(model, onImported = onOpen, onFailure = { alert = it }, onImporting = { importing = it })
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Library") },
                actions = {
                    FilterAndSortMenu(model)
                    IconButton(onClick = onSettings, modifier = Modifier.testTag("library.settings")) {
                        Icon(Icons.Filled.Settings, contentDescription = "Settings")
                    }
                    AddMenu(model, tag = "library.add", onAdd = onAdd, onFind = onFind, onImport = onImport) { open ->
                        IconButton(onClick = open, modifier = Modifier.testTag("library.add")) {
                            Icon(Icons.Filled.Add, contentDescription = "Add Book")
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (importing) LinearProgressIndicator(Modifier.fillMaxWidth().testTag("library.importing"))
            if (all.isNotEmpty()) SearchField(search, model::setSearchText)
            when {
                all.isEmpty() -> Message(
                    "No Books Yet",
                    "Add the books you're reading, have read, or want to read.",
                    tag = "library.empty",
                ) {
                    AddMenu(model, tag = "library.empty.add", onAdd = onAdd, onFind = onFind, onImport = onImport) { open ->
                        Button(onClick = open, modifier = Modifier.testTag("library.empty.add")) { Text("Add Book") }
                    }
                }
                books.isEmpty() && search.isEmpty() -> {
                    val label = filter?.let(LibraryModel::statusLabel) ?: ""
                    Message("No $label Books", "Books you mark as $label appear here.", tag = "library.filtered.empty")
                }
                books.isEmpty() -> Message(
                    "No Results for “$search”",
                    "Check the spelling or try a new search.",
                    tag = "library.noResults",
                )
                else -> LazyColumn(Modifier.fillMaxSize()) {
                    items(books, key = { it.id.toLong() }) { book ->
                        BookRow(book, onClick = { onOpen(book.id) })
                        HorizontalDivider()
                    }
                }
            }
        }
    }
    ForgeAlertDialog(alert, onDismiss = { alert = null }) {}
}

/**
 * The ways to add a book, for the toolbar and the empty library: [button] opens the menu. With
 * online search off, the catalogue choice is disabled and says why (PL-7).
 */
@Composable
private fun AddMenu(
    model: LibraryModel,
    tag: String,
    onAdd: () -> Unit,
    onFind: () -> Unit,
    onImport: () -> Unit,
    button: @Composable (open: () -> Unit) -> Unit,
) {
    val settings by model.settings.collectAsState()
    var open by remember { mutableStateOf(false) }
    Box {
        button { open = true }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, modifier = Modifier.testTag("$tag.menu")) {
            DropdownMenuItem(
                text = {
                    Column {
                        Text("Search Online Catalogue")
                        if (!settings.catalogueEnabled) {
                            Text(
                                "Online search is off in Settings",
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.testTag("library.add.catalogue.off"),
                            )
                        }
                    }
                },
                onClick = { open = false; onFind() },
                enabled = settings.catalogueEnabled,
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                modifier = Modifier.testTag("library.add.catalogue"),
            )
            DropdownMenuItem(
                text = { Text("Enter Details") },
                onClick = { open = false; onAdd() },
                leadingIcon = { Icon(Icons.Filled.Edit, contentDescription = null) },
                modifier = Modifier.testTag("library.add.manual"),
            )
            DropdownMenuItem(
                text = { Text("Import EPUB File") },
                onClick = { open = false; onImport() },
                leadingIcon = { Icon(Icons.Filled.Add, contentDescription = null) },
                modifier = Modifier.testTag("library.add.epub"),
            )
        }
    }
}

/** Searches title and author as the person types; Rust does the matching. */
@Composable
private fun SearchField(text: String, onChange: (String) -> Unit) {
    val focus = LocalFocusManager.current
    OutlinedTextField(
        value = text,
        onValueChange = onChange,
        placeholder = { Text("Search") },
        leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
        trailingIcon = {
            if (text.isNotEmpty()) {
                IconButton(onClick = { onChange("") }, modifier = Modifier.testTag("library.search.clear")) {
                    Icon(Icons.Filled.Clear, contentDescription = "Clear Search")
                }
            }
        },
        singleLine = true,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { focus.clearFocus() }),
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = Forge.spacing.l.dp, vertical = Forge.spacing.s.dp)
            .testTag("library.search"),
    )
}

/**
 * Filter and sort in one menu, as on the iPhone. Choices are `library.filter.<status>` (or
 * `.all`) and `library.sort.<order>`.
 */
@Composable
private fun FilterAndSortMenu(model: LibraryModel) {
    val filter by model.statusFilter.collectAsState()
    val sort by model.sort.collectAsState()
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.testTag("library.filter")) {
            Icon(
                Icons.AutoMirrored.Filled.List,
                contentDescription = if (filter == null) "Filter and Sort" else "Filter and Sort, showing ${LibraryModel.statusLabel(filter!!)}",
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            MenuHeading("Show")
            Choice("All Books", filter == null, "library.filter.all") { model.setStatusFilter(null); open = false }
            for (status in ReadingStatus.entries) {
                Choice(LibraryModel.statusLabel(status), filter == status, "library.filter.${status.key()}") {
                    model.setStatusFilter(status)
                    open = false
                }
            }
            HorizontalDivider()
            MenuHeading("Sort By")
            for (order in LibrarySort.entries) {
                Choice(LibraryModel.sortLabel(order), sort == order, "library.sort.${order.key()}") {
                    model.setSort(order)
                    open = false
                }
            }
        }
    }
}

private fun LibrarySort.key(): String = when (this) {
    LibrarySort.RECENTLY_ADDED -> "recentlyAdded"
    LibrarySort.TITLE -> "title"
    LibrarySort.AUTHOR -> "author"
    LibrarySort.PROGRESS -> "progress"
}

@Composable
private fun MenuHeading(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
    )
}

@Composable
private fun Choice(label: String, selected: Boolean, tag: String, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label) },
        onClick = onClick,
        leadingIcon = {
            if (selected) Icon(Icons.Filled.Check, contentDescription = "Selected") else Box(Modifier.width(24.dp))
        },
        modifier = Modifier.testTag(tag),
    )
}

@Composable
private fun Message(title: String, detail: String, tag: String, action: @Composable () -> Unit = {}) {
    Centered(Modifier.padding(24.dp)) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(title, style = Forge.text(ForgeTextRole.SECTION_TITLE), modifier = Modifier.testTag(tag))
            Text(detail, style = Forge.text(ForgeTextRole.METADATA))
            action()
        }
    }
}

@Composable
private fun BookRow(book: Book, onClick: () -> Unit) {
    val tone = Forge.colors.accent
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(onClickLabel = "Open ${book.title}", onClick = onClick)
            .testTag("library.book.${book.id}")
            .padding(horizontal = Forge.spacing.l.dp, vertical = Forge.spacing.m.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                book.title,
                style = Forge.text(ForgeTextRole.ROW_TITLE),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            book.rating?.let { stars ->
                Text(
                    "★".repeat(stars.toInt()),
                    style = MaterialTheme.typography.labelSmall,
                    color = Forge.colors.warning.text.toColor(),
                    modifier = Modifier.semantics { contentDescription = "$stars stars" },
                )
            }
        }
        Text(
            book.author.ifEmpty { "Unknown author" },
            style = Forge.text(ForgeTextRole.METADATA),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            if (book.status == ReadingStatus.READING && book.totalPages != null) {
                LinearProgressIndicator(
                    progress = { book.progress.toFloat() },
                    color = tone.solid.toColor(),
                    modifier = Modifier.width(90.dp),
                )
            }
            Text(
                "${LibraryModel.statusLabel(book.status)} · ${LibraryModel.pageSummary(book)}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** The reading status as an accent badge (ForgeBadge on iOS). */
@Composable
internal fun StatusBadge(label: String, modifier: Modifier = Modifier) {
    val accent = Forge.colors.accent
    Text(
        label,
        style = MaterialTheme.typography.labelLarge,
        color = accent.text.toColor(),
        modifier = modifier
            .background(accent.soft.compositedOver(Forge.colors.background).toColor(), RoundedCornerShape(Forge.radius.s.dp))
            .padding(horizontal = Forge.spacing.s.dp, vertical = Forge.spacing.xs.dp),
    )
}

@Composable
internal fun Centered(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}
