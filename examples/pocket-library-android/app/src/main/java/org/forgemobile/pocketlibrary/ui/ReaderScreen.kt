// Reads a book's EPUB, as the iPhone app's ReaderView does: one chapter at a time, scrolling,
// with the text laid out natively (so it follows the system's font size and dark theme, and the
// reader's own text settings). Rust parses the EPUB, keeps the place as a block anchor and
// decides what it means for the book's page; this screen reports which block is at the top.
// Read Aloud speaks exactly the text shown, block by block, and the screen follows the voice.
package org.forgemobile.pocketlibrary.ui

import android.content.Context
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlin.coroutines.cancellation.CancellationException
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.forgemobile.pocketlibrary.LibraryModel
import org.forgemobile.pocketlibrary.kit.ForgeAlert
import org.forgemobile.pocketlibrary.kit.ForgeAlertDialog
import org.forgemobile.pocketlibrary.kit.ForgeFailure
import org.forgemobile.pocketlibrary.reader.ReadAloud
import org.forgemobile.pocketlibrary.reader.ReadAloudSpeaker
import org.forgemobile.pocketlibrary.ui.theme.Forge
import org.forgemobile.pocketlibrary.ui.theme.ForgeTextRole
import org.forgemobile.pocketlibrary.ui.theme.toColor
import uniffi.pocket_library_core.ChapterSummary
import uniffi.pocket_library_core.ReaderBlock
import uniffi.pocket_library_core.ReaderBlockKind
import uniffi.pocket_library_core.ReaderChapter
import uniffi.pocket_library_core.ReaderContents
import uniffi.pocket_library_core.ReadingStatus

/** A block of a chapter: what the reader last saved. */
private data class SavedPlace(val chapter: UInt, val block: Int)

/** What the reader keeps between compositions without drawing it. */
private class ReaderSession {
    /** The place last asked to be saved; null after a failed save, so the next one is written. */
    var lastSaved: SavedPlace? = null

    /** The first chapter has loaded (the place it opened at needs no save). */
    var opened = false

    /** The latest save, which a chapter load waits for (Rust saves only the chapter loaded last). */
    var saving: Job? = null

    /** Counts chapter loads: only the latest one is shown. */
    var loads = 0

    /** Chapter loads run one at a time, so the chapter Rust loaded last is the one shown. */
    val loading = Mutex()

    /** The reader has closed: failures are no longer shown. */
    var closed = false
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ReaderScreen(model: LibraryModel, bookId: ULong, onClose: () -> Unit) {
    val context = LocalContext.current
    // Screen state changes on the main thread, whatever thread a Rust call resumes on.
    val scope = rememberCoroutineScope { Dispatchers.Main.immediate }
    val readAloud = remember { ReadAloud(ReadAloudSpeaker.make(context)) }
    val session = remember { ReaderSession() }
    val text = rememberReaderTextSettings(context)

    var contents by remember { mutableStateOf<ReaderContents?>(null) }
    var chapter by remember { mutableStateOf<ReaderChapter?>(null) }
    var openError by remember { mutableStateOf<ForgeFailure?>(null) }
    var alert by remember { mutableStateOf<ForgeAlert<Nothing>?>(null) }
    var loadingChapter by remember { mutableStateOf(false) }
    var showingContents by remember { mutableStateOf(false) }
    // Each chapter load gets a fresh list, starting at the block the chapter opens at.
    var loadCount by remember { mutableIntStateOf(0) }
    var startBlock by remember { mutableIntStateOf(0) }
    val listState = remember(loadCount) { LazyListState(firstVisibleItemIndex = startBlock) }
    /** The block at the top of the screen (the chapter's end is `blocks.size`). */
    val topBlock by remember(listState) { derivedStateOf { listState.firstVisibleItemIndex } }

    val speaking by readAloud.state.collectAsState()
    val spokenBlock by readAloud.block.collectAsState()
    val spokenWord by readAloud.word.collectAsState()
    val speed by readAloud.speed.collectAsState()
    val readAloudFailed by readAloud.failed.collectAsState()
    val books by model.all.collectAsState()
    val book = books.firstOrNull { it.id == bookId }

    fun savePosition(block: Int? = null) {
        val current = chapter ?: return
        if (loadingChapter) return
        val place = SavedPlace(current.index, block ?: listState.firstVisibleItemIndex)
        if (place == session.lastSaved) return
        session.lastSaved = place
        // In the model's scope: moving to another chapter, or closing, doesn't cancel it.
        session.saving = model.saveReadingPositionLater(bookId, place.chapter, place.block) { e ->
            // Not saved: forget it, so the next move, or leaving, writes again whatever the place.
            if (session.lastSaved == place) session.lastSaved = null
            if (!session.closed) alert = ForgeAlert.failure(e)
        }
    }

    /**
     * Loads chapter [index]: at its top, or at the saved place the first time the chapter the
     * book opens at is loaded (Rust finds the saved anchor).
     */
    suspend fun show(index: UInt) {
        savePosition()
        val load = ++session.loads
        loadingChapter = true
        try {
            val loaded = session.loading.withLock {
                // The place in this chapter is saved before Rust loads another.
                session.saving?.join()
                model.chapter(bookId, index)
            }
            // A later load (a second tap) wins.
            if (load != session.loads) return
            val last = maxOf(loaded.section.blocks.size - 1, 0)
            val start = minOf(loaded.start?.block?.toInt() ?: 0, last)
            if (!session.opened) {
                session.opened = true
                session.lastSaved = SavedPlace(index, start)
            }
            startBlock = start
            chapter = loaded
            openError = null
            loadCount += 1
            // Reading aloud carries on into the new chapter.
            val wasPlaying = readAloud.state.value == ReadAloud.State.PLAYING
            if (readAloud.state.value == ReadAloud.State.PAUSED) readAloud.stop()
            readAloud.load(loaded.section.blocks)
            if (wasPlaying) readAloud.play(start)
        } catch (e: Exception) {
            if (e is CancellationException) throw e
            if (load != session.loads) return
            // Reading aloud was handing over to this chapter: it stops rather than hang.
            if (readAloud.state.value != ReadAloud.State.STOPPED && readAloud.block.value == null) readAloud.stop()
            if (contents == null || chapter == null) openError = ForgeFailure.from(e) else alert = ForgeAlert.failure(e)
        } finally {
            if (load == session.loads) loadingChapter = false
        }
    }

    fun move(index: UInt) {
        scope.launch { show(index) }
    }

    LaunchedEffect(bookId) {
        try {
            val opened = model.openReader(bookId)
            contents = opened
            show(opened.start.chapter)
        } catch (e: Exception) {
            if (e is CancellationException) throw e
            openError = ForgeFailure.from(e)
        }
    }

    // The voice saves the place as it moves (it may be ahead of the screen), and moves on to the
    // next chapter at the end of one. Set on every composition, so they see the current chapter.
    SideEffect {
        readAloud.onBlock = { block -> savePosition(block) }
        readAloud.onChapterEnd = {
            val current = chapter
            val count = contents?.chapters?.size ?: 0
            when {
                current == null || current.index.toInt() + 1 >= count -> readAloud.stop()
                // A chapter is already loading (a tap on the bar or the contents): reading
                // carries on into that one.
                loadingChapter -> Unit
                else -> move(current.index + 1u)
            }
        }
    }

    // Save a moment after scrolling stops.
    LaunchedEffect(chapter?.index, topBlock) {
        delay(1_500)
        savePosition()
    }

    // Follow the voice: the block being read goes to the top.
    LaunchedEffect(spokenBlock, loadCount) {
        val block = spokenBlock ?: return@LaunchedEffect
        if (readAloud.state.value == ReadAloud.State.PLAYING) listState.animateScrollToItem(block)
    }

    LaunchedEffect(readAloudFailed) {
        if (!readAloudFailed) return@LaunchedEffect
        alert = ForgeAlert.failure(
            ForgeFailure(
                "Can't Read Aloud",
                "No text-to-speech voice is ready. Choose or install one in Settings › System › Languages › Text-to-speech output.",
            ),
        )
        readAloud.clearFailure()
    }

    // Save when the app goes to the background, and when the reader closes (in the model's
    // scope, which outlives this screen).
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val currentSave by rememberUpdatedState(::savePosition)
    val leave by rememberUpdatedState { spoken: Int? ->
        val current = chapter
        if (current != null && !loadingChapter) {
            // While reading aloud, the voice's block is the place (it may be ahead of the screen).
            val place = SavedPlace(current.index, spoken ?: listState.firstVisibleItemIndex)
            if (place != session.lastSaved) model.saveReadingPositionLater(bookId, place.chapter, place.block)
        }
    }
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_STOP) currentSave(null) }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            session.closed = true
            val spoken = readAloud.block.value.takeIf { readAloud.state.value != ReadAloud.State.STOPPED }
            readAloud.close()
            leave(spoken)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        chapter?.section?.title ?: contents?.title ?: "",
                        maxLines = 1,
                        modifier = Modifier.testTag("reader.title"),
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onClose, modifier = Modifier.testTag("reader.close")) {
                        Icon(Icons.Filled.Close, contentDescription = "Close Reader")
                    }
                },
                actions = {
                    TextSettingsMenu(text)
                    IconButton(
                        onClick = { showingContents = true },
                        enabled = contents != null,
                        modifier = Modifier.testTag("reader.contents"),
                    ) { Icon(Icons.AutoMirrored.Filled.List, contentDescription = "Contents") }
                    IconButton(
                        onClick = { readAloud.play(topBlock) },
                        enabled = chapter != null && speaking == ReadAloud.State.STOPPED,
                        modifier = Modifier.testTag("reader.readAloud"),
                    ) { Icon(ReaderIcons.Headphones, contentDescription = "Read Aloud") }
                },
            )
        },
        bottomBar = {
            val shownContents = contents
            val shownChapter = chapter
            if (shownContents != null && shownChapter != null) {
                Surface(tonalElevation = 3.dp) {
                    Column(Modifier.navigationBarsPadding()) {
                        if (speaking != ReadAloud.State.STOPPED) {
                            Player(readAloud, speaking, speed, from = { topBlock })
                            HorizontalDivider()
                        }
                        ProgressBar(
                            shownContents,
                            shownChapter,
                            topBlock,
                            loading = loadingChapter,
                            onMove = ::move,
                        )
                    }
                }
            }
        },
    ) { padding ->
        val shownContents = contents
        val shownChapter = chapter
        val failure = openError
        when {
            failure != null -> Centered(Modifier.padding(padding).padding(24.dp)) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(failure.title, style = Forge.text(ForgeTextRole.SECTION_TITLE), modifier = Modifier.testTag("reader.error"))
                    Text(failure.message, style = Forge.text(ForgeTextRole.METADATA), textAlign = TextAlign.Center)
                }
            }
            shownContents != null && shownChapter != null -> LazyColumn(
                state = listState,
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                modifier = Modifier.padding(padding).fillMaxSize().testTag("reader.text"),
            ) {
                itemsIndexed(shownChapter.section.blocks) { index, block ->
                    Box(Modifier.fillMaxWidth().wrapContentWidth().widthIn(max = 640.dp)) {
                        ReaderBlockView(block, index, text, spokenWord = if (spokenBlock == index) spokenWord else null)
                    }
                }
                item {
                    ChapterEnd(
                        next = shownContents.chapters.firstOrNull { it.index == shownChapter.index + 1u },
                        finished = book?.status == ReadingStatus.FINISHED,
                        loading = loadingChapter,
                        onNext = ::move,
                        onFinish = {
                            scope.launch {
                                try {
                                    model.setStatus(bookId, ReadingStatus.FINISHED)
                                } catch (e: Exception) {
                                    if (e is CancellationException) throw e
                                    alert = ForgeAlert.failure(e)
                                }
                            }
                        },
                    )
                }
            }
            else -> Centered(Modifier.padding(padding)) { CircularProgressIndicator(Modifier.testTag("reader.loading")) }
        }
    }

    val shownContents = contents
    if (showingContents && shownContents != null) {
        ContentsSheet(
            shownContents,
            current = chapter?.index ?: 0u,
            onSelect = { index ->
                showingContents = false
                move(index)
            },
            onDismiss = { showingContents = false },
        )
    }
    ForgeAlertDialog(alert, onDismiss = { alert = null }) {}
}

// MARK: Text settings

/** Steps larger or smaller than the system's text size, and serif or system font, kept for every book. */
private class ReaderTextSettings(private val context: Context, step: Int, serif: Boolean) {
    var step by mutableIntStateOf(step)
        private set
    var serif by mutableStateOf(serif)
        private set

    /** How much larger than the system text size: about 15% a step. */
    val scale: Float get() = 1.15f.pow(step)

    fun changeStep(to: Int) {
        step = to.coerceIn(MIN_STEP, MAX_STEP)
        prefs(context).edit().putInt(STEP_KEY, step).apply()
    }

    fun changeSerif(to: Boolean) {
        serif = to
        prefs(context).edit().putBoolean(SERIF_KEY, to).apply()
    }

    companion object {
        const val MIN_STEP = -3
        const val MAX_STEP = 6

        // The iPhone app's keys (ForgeReaderSettings), in the app's own preferences file.
        private const val STEP_KEY = "forgeReader.textStep"
        private const val SERIF_KEY = "forgeReader.serif"

        fun prefs(context: Context) = context.getSharedPreferences("reader", Context.MODE_PRIVATE)

        fun load(context: Context): ReaderTextSettings {
            val prefs = prefs(context)
            return ReaderTextSettings(context.applicationContext, prefs.getInt(STEP_KEY, 0), prefs.getBoolean(SERIF_KEY, true))
        }
    }
}

@Composable
private fun rememberReaderTextSettings(context: Context) = remember { ReaderTextSettings.load(context) }

/** Larger, smaller and system text, and the font: `reader.textSettings`. */
@Composable
private fun TextSettingsMenu(settings: ReaderTextSettings) {
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(
            onClick = { open = true },
            modifier = Modifier.testTag("reader.textSettings").semantics { contentDescription = "Text Settings" },
        ) { Text("Aa") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("Larger Text") },
                onClick = { settings.changeStep(settings.step + 1) },
                enabled = settings.step < ReaderTextSettings.MAX_STEP,
                modifier = Modifier.testTag("reader.textSettings.larger"),
            )
            DropdownMenuItem(
                text = { Text("Smaller Text") },
                onClick = { settings.changeStep(settings.step - 1) },
                enabled = settings.step > ReaderTextSettings.MIN_STEP,
                modifier = Modifier.testTag("reader.textSettings.smaller"),
            )
            if (settings.step != 0) {
                DropdownMenuItem(
                    text = { Text("System Text Size") },
                    onClick = { settings.changeStep(0) },
                    modifier = Modifier.testTag("reader.textSettings.system"),
                )
            }
            HorizontalDivider()
            for ((label, serif) in listOf("Serif" to true, "System" to false)) {
                DropdownMenuItem(
                    text = { Text(label) },
                    onClick = { settings.changeSerif(serif); open = false },
                    leadingIcon = {
                        if (settings.serif == serif) Icon(Icons.Filled.Check, contentDescription = "Selected") else Box(Modifier.width(24.dp))
                    },
                    modifier = Modifier.testTag("reader.textSettings.${label.lowercase()}"),
                )
            }
        }
    }
}

// MARK: Blocks

/**
 * One paragraph, heading, quote, list item, scene break or picture, tagged `reader.block.<index>`.
 * The text is the block's spans exactly, so Read Aloud's word positions line up with it.
 */
@Composable
private fun ReaderBlockView(block: ReaderBlock, index: Int, settings: ReaderTextSettings, spokenWord: IntRange?) {
    val family = if (settings.serif) FontFamily.Serif else FontFamily.Default
    val type = MaterialTheme.typography
    val body = type.bodyLarge.copy(fontFamily = family, lineHeight = type.bodyLarge.fontSize * 1.5f).scaled(settings.scale)
    val highlight = Forge.colors.accent.solid.toColor().copy(alpha = 0.3f)
    val link = Forge.colors.accent.text.toColor()
    val tag = Modifier.testTag("reader.block.$index")
    when (val kind = block.kind) {
        is ReaderBlockKind.Heading -> {
            val style = when (kind.level.toInt()) {
                1 -> type.headlineSmall
                2 -> type.titleLarge
                3 -> type.titleMedium
                else -> type.titleSmall
            }.copy(fontFamily = family, fontWeight = FontWeight.SemiBold).scaled(settings.scale)
            Text(
                annotated(block, spokenWord, highlight, link),
                style = style,
                modifier = tag.padding(top = if (kind.level <= 2u) 16.dp else 8.dp).semantics { heading() },
            )
        }
        ReaderBlockKind.Paragraph -> Text(annotated(block, spokenWord, highlight, link), style = body, modifier = tag)
        ReaderBlockKind.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.width(3.dp).fillMaxHeight().background(MaterialTheme.colorScheme.outlineVariant))
            Spacer(Modifier.width(13.dp))
            Text(
                annotated(block, spokenWord, highlight, link),
                style = body,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = tag,
            )
        }
        is ReaderBlockKind.ListItem -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            // Shown beside the item, not read out.
            Text(kind.marker, style = body, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.clearAndSetSemantics {})
            Text(annotated(block, spokenWord, highlight, link), style = body, modifier = tag)
        }
        ReaderBlockKind.Preformatted -> Box(Modifier.horizontalScroll(rememberScrollState())) {
            Text(
                annotated(block, spokenWord, highlight, link),
                style = type.bodyMedium.copy(fontFamily = FontFamily.Monospace).scaled(settings.scale),
                softWrap = false,
                modifier = tag,
            )
        }
        ReaderBlockKind.Separator -> Text(
            "⁂",
            style = type.titleLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = tag.fillMaxWidth().clearAndSetSemantics { contentDescription = "Section break" },
        )
        is ReaderBlockKind.Image -> ReaderImage(kind.source, kind.alt, tag)
    }
}

private fun TextStyle.scaled(scale: Float): TextStyle =
    if (scale == 1f) this else copy(fontSize = fontSize * scale, lineHeight = lineHeight * scale)

/** The block's spans as styled text, with the word being read aloud highlighted. */
private fun annotated(
    block: ReaderBlock,
    spokenWord: IntRange?,
    highlight: androidx.compose.ui.graphics.Color,
    linkColor: androidx.compose.ui.graphics.Color,
): AnnotatedString = buildAnnotatedString {
    for (span in block.spans) {
        val style = SpanStyle(
            fontWeight = if (span.bold) FontWeight.Bold else null,
            fontStyle = if (span.italic) FontStyle.Italic else null,
            fontFamily = if (span.monospace) FontFamily.Monospace else null,
        )
        val url = span.link?.takeIf { it.startsWith("https://") || it.startsWith("http://") }
        if (url != null) {
            val linkStyle = TextLinkStyles(SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline))
            withLink(LinkAnnotation.Url(url, linkStyle)) { withStyle(style) { append(span.text) } }
        } else {
            withStyle(style) { append(span.text) }
        }
    }
    if (spokenWord != null) {
        val start = spokenWord.first.coerceIn(0, length)
        val end = (spokenWord.last + 1).coerceIn(start, length)
        if (end > start) addStyle(SpanStyle(background = highlight), start, end)
    }
}

/** A picture from the EPUB (a file Rust saved), scaled to the text column. */
@Composable
private fun ReaderImage(source: String, alt: String, modifier: Modifier) {
    // A new source starts empty, rather than showing the previous picture while it decodes.
    var image by remember(source) { mutableStateOf<ImageBitmap?>(null) }
    LaunchedEffect(source) {
        image = withContext(Dispatchers.IO) { decodeScaled(source, maxPixels = 1600) }
    }
    val bitmap = image
    val described = modifier.fillMaxWidth().semantics { if (alt.isNotEmpty()) contentDescription = alt }
    if (bitmap != null) {
        Image(bitmap, contentDescription = null, contentScale = ContentScale.FillWidth, modifier = described)
    } else {
        Box(described.height(120.dp))
    }
}

private fun decodeScaled(path: String, maxPixels: Int): ImageBitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(path, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxPixels) sample *= 2
    val options = BitmapFactory.Options().apply { inSampleSize = sample }
    return BitmapFactory.decodeFile(path, options)?.asImageBitmap()
}

// MARK: Around the text

/** The next chapter, or The End with Mark as Finished. */
@Composable
private fun ChapterEnd(
    next: ChapterSummary?,
    finished: Boolean,
    loading: Boolean,
    onNext: (UInt) -> Unit,
    onFinish: () -> Unit,
) {
    Column(
        Modifier.fillMaxWidth().padding(vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        HorizontalDivider()
        if (next != null) {
            Button(onClick = { onNext(next.index) }, enabled = !loading, modifier = Modifier.testTag("reader.nextChapterEnd")) {
                Text("Next: ${next.title}", textAlign = TextAlign.Center)
            }
        } else {
            Text("The End", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("reader.end"))
            if (!finished) {
                Button(onClick = onFinish, modifier = Modifier.testTag("reader.markFinished")) {
                    Icon(Icons.Filled.CheckCircle, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("Mark as Finished")
                }
            }
        }
    }
}

/** "Chapter 8 of 50 · 23%", with previous and next. */
@Composable
private fun ProgressBar(
    contents: ReaderContents,
    chapter: ReaderChapter,
    topBlock: Int,
    loading: Boolean,
    onMove: (UInt) -> Unit,
) {
    val count = contents.chapters.size
    val index = chapter.index.toInt()
    val fraction = bookFraction(contents, chapter, topBlock)
    Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(
            onClick = { onMove(chapter.index - 1u) },
            enabled = index > 0 && !loading,
            modifier = Modifier.testTag("reader.previousChapter"),
        ) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = "Previous Chapter") }
        Column(
            // One element for screen readers; the tag is here, as merging hides the text's own node.
            Modifier.weight(1f).semantics(mergeDescendants = true) {}.testTag("reader.progress"),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            LinearProgressIndicator(
                progress = { fraction.toFloat() },
                color = Forge.colors.accent.solid.toColor(),
                modifier = Modifier.widthIn(max = 280.dp).fillMaxWidth(),
            )
            Text(
                "Chapter ${index + 1} of $count · ${(fraction * 100).roundToInt()}%",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(
            onClick = { onMove(chapter.index + 1u) },
            enabled = index + 1 < count && !loading,
            modifier = Modifier.testTag("reader.nextChapter"),
        ) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = "Next Chapter") }
    }
}

/** How far through the book the top of the screen is, by words (for the bar only; Rust works out the page). */
private fun bookFraction(contents: ReaderContents, chapter: ReaderChapter, topBlock: Int): Double {
    if (contents.totalWords == 0uL) return 0.0
    val index = chapter.index.toInt()
    val before = contents.chapters.take(index).sumOf { it.words.toDouble() }
    val blocks = chapter.section.blocks.size
    val inChapter = if (blocks == 0) 0.0 else minOf(topBlock.toDouble() / blocks, 1.0)
    val here = (contents.chapters.getOrNull(index)?.words?.toDouble() ?: 0.0) * inChapter
    return minOf((before + here) / contents.totalWords.toDouble(), 1.0)
}

/** Read Aloud's controls: previous and next paragraph, play or pause, speed, stop. */
@Composable
private fun Player(readAloud: ReadAloud, state: ReadAloud.State, speed: Double, from: () -> Int) {
    var choosingSpeed by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = { readAloud.skip(-1) }, modifier = Modifier.testTag("reader.player.previous")) {
            Icon(ReaderIcons.FastRewind, contentDescription = "Previous Paragraph")
        }
        IconButton(onClick = { readAloud.toggle(from()) }, modifier = Modifier.testTag("reader.player.toggle")) {
            if (state == ReadAloud.State.PLAYING) {
                Icon(ReaderIcons.Pause, contentDescription = "Pause")
            } else {
                Icon(Icons.Filled.PlayArrow, contentDescription = "Play")
            }
        }
        IconButton(onClick = { readAloud.skip(1) }, modifier = Modifier.testTag("reader.player.next")) {
            Icon(ReaderIcons.FastForward, contentDescription = "Next Paragraph")
        }
        Box {
            TextButton(
                onClick = { choosingSpeed = true },
                modifier = Modifier.testTag("reader.player.speed").semantics { contentDescription = "Speed, ${ReadAloud.speedLabel(speed)}" },
            ) { Text(ReadAloud.speedLabel(speed)) }
            DropdownMenu(expanded = choosingSpeed, onDismissRequest = { choosingSpeed = false }) {
                for (choice in ReadAloud.SPEEDS) {
                    val label = ReadAloud.speedLabel(choice)
                    DropdownMenuItem(
                        text = { Text(label) },
                        onClick = { readAloud.setSpeed(choice); choosingSpeed = false },
                        leadingIcon = {
                            if (choice == speed) Icon(Icons.Filled.Check, contentDescription = "Selected") else Box(Modifier.width(24.dp))
                        },
                        modifier = Modifier.testTag("reader.player.speed.$choice"),
                    )
                }
            }
        }
        IconButton(onClick = { readAloud.stop() }, modifier = Modifier.testTag("reader.player.stop")) {
            Icon(Icons.Filled.Close, contentDescription = "Stop Reading Aloud")
        }
    }
}

/** The book's table of contents; picking a chapter opens it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ContentsSheet(contents: ReaderContents, current: UInt, onSelect: (UInt) -> Unit, onDismiss: () -> Unit) {
    // The chapters the book lists, or every chapter if it lists none.
    val entries = contents.chapters.filter { it.inContents }.ifEmpty { contents.chapters }
    // The entry the current chapter belongs to.
    val currentEntry = entries.lastOrNull { it.index <= current }?.index
    val list = rememberLazyListState(initialFirstVisibleItemIndex = maxOf(entries.indexOfFirst { it.index == currentEntry } - 2, 0))
    ModalBottomSheet(onDismissRequest = onDismiss, modifier = Modifier.testTag("contents")) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Contents", style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
            TextButton(onClick = onDismiss, modifier = Modifier.testTag("contents.done")) { Text("Done") }
        }
        LazyColumn(state = list, modifier = Modifier.fillMaxWidth()) {
            itemsIndexed(entries) { _, entry ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { onSelect(entry.index) }
                        .testTag("contents.chapter.${entry.index}")
                        .padding(horizontal = 16.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(entry.title, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    if (entry.index == currentEntry) {
                        Icon(Icons.Filled.Check, contentDescription = "Current chapter", tint = Forge.colors.accent.solid.toColor())
                    }
                }
                HorizontalDivider()
            }
        }
    }
}
