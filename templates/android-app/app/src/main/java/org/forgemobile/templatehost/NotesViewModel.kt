package FORGE_JAVA_PACKAGE

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import kotlinx.coroutines.CancellationException
import uniffi.forge_ffi.AppCore
import uniffi.forge_ffi.AppEvent
import uniffi.forge_ffi.AppEvents
import uniffi.forge_ffi.AppException
import uniffi.forge_ffi.Note

/**
 * Presentation state for the notes screen.
 *
 * Holds no app rules: every change goes to the Rust `AppCore` (your code in `rust/src/lib.rs`),
 * and the list shown comes from Rust, through a command's result or a `NotesChanged` event. The
 * commands are `suspend` functions; Rust does slow work on its own threads, so calling them from
 * the main thread never blocks it.
 *
 * @param storageDirectory where Rust keeps `notes.json`. [NotesViewModelFactory][FORGE_JAVA_PACKAGE.ui.NotesViewModelFactory]
 *   passes a folder under the app's files dir; tests pass a temporary folder.
 */
class NotesViewModel(private val storageDirectory: String) : ViewModel() {
    var notes by mutableStateOf<List<Note>>(emptyList())
        private set
    var draft by mutableStateOf("")
    var url by mutableStateOf("https://example.com")
    var urlResult by mutableStateOf<String?>(null)
        private set
    var isCheckingUrl by mutableStateOf(false)
        private set
    var errorMessage by mutableStateOf<String?>(null)
        private set
    /** Set when the app could not be opened (for example unreadable data). */
    var openError by mutableStateOf<String?>(null)
        private set

    var core: AppCore? = null
        private set
    private var events: AppEvents? = null

    init {
        open()
    }

    /** Opens the Rust core. Does nothing when already open. */
    fun open() {
        if (core != null) return
        openError = null
        try {
            val opened = AppCore(storageDirectory)
            // Subscribe before reading, so no change is missed in between.
            events = opened.events()
            core = opened
            notes = opened.notes()
        } catch (e: AppException) {
            openError = describe(e)
        }
    }

    /**
     * Applies Rust's events until the stream ends ([close]) or the caller is cancelled. The screen
     * runs it in a `LaunchedEffect`.
     */
    suspend fun listen() {
        val stream = events ?: return
        while (true) {
            when (val event = stream.next() ?: return) {
                is AppEvent.NotesChanged -> notes = event.notes
                // Missed some events: the current list is the truth.
                is AppEvent.EventsDropped -> core?.let { notes = it.notes() }
            }
        }
    }

    suspend fun addNote() {
        val current = core ?: return
        errorMessage = null
        try {
            current.addNote(draft)
            draft = ""
            // The list also updates from the event; reading it here shows the change at once.
            notes = current.notes()
        } catch (e: AppException) {
            errorMessage = describe(e)
        }
    }

    suspend fun delete(note: Note) {
        val current = core ?: return
        errorMessage = null
        try {
            current.deleteNote(note.id)
            notes = current.notes()
        } catch (e: AppException) {
            errorMessage = describe(e)
        }
    }

    /**
     * Fetches [url] from Rust; HTTPS certificates are checked against Android's trust store.
     * Cancelling the calling coroutine stops the request in Rust at once (UniFFI drops the Rust
     * future, which cancels it), even while it waits for a slow server.
     */
    suspend fun checkUrl() {
        val current = core ?: return
        if (isCheckingUrl) return
        isCheckingUrl = true
        urlResult = null
        try {
            urlResult = "HTTP ${current.checkUrl(url, null)}"
        } catch (e: AppException) {
            urlResult = describe(e)
        } catch (e: CancellationException) {
            urlResult = "Cancelled."
            throw e
        } finally {
            isCheckingUrl = false
        }
    }

    /** Ends the event stream and shuts the Rust core down. Saved notes are already on disk. */
    fun close() {
        events?.cancel()
        events?.close()
        events = null
        core?.shutdown()
        core?.close()
        core = null
    }

    override fun onCleared() {
        close()
        super.onCleared()
    }

    companion object {
        /** Text for the screen. Replace with your own copy; `reason` fields are diagnostics. */
        fun describe(error: AppException): String =
            when (error) {
                is AppException.InvalidInput -> "Check your input: ${error.reason}"
                is AppException.NotFound -> "That note no longer exists."
                is AppException.Storage -> "Could not save: ${error.reason}"
                is AppException.Offline -> "You appear to be offline."
                is AppException.Certificate ->
                    "The server's certificate was rejected: ${error.reason}"
                is AppException.Timeout -> "The request timed out."
                is AppException.Network -> "Network error: ${error.reason}"
                is AppException.Cancelled -> "Cancelled."
                is AppException.ShutDown -> "The app is closing."
            }
    }
}
