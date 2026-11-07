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
import uniffi.forge_ffi.AppPhase
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
    /** The lifecycle phase last reported to Rust. */
    var phase by mutableStateOf(AppPhase.LAUNCHING)
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
            // A phase reported before Rust was open (a retry after an error) still counts.
            if (phase != AppPhase.LAUNCHING) opened.setPhase(phase)
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
                // Back from the background: re-read what Rust holds. A real app would also
                // restart work the background stopped.
                is AppEvent.PhaseChanged -> if (event.resumed) core?.let { notes = it.notes() }
            }
        }
    }

    /**
     * Tells Rust where the app is in its lifecycle. [MainActivity] calls it through
     * `reportLifecycle`. Rust stops on-screen work in the background and sends
     * `PhaseChanged(resumed = true)` on the way back. `TERMINATED` shuts the core down, like
     * [close].
     */
    fun setPhase(phase: AppPhase) {
        this.phase = phase
        if (phase == AppPhase.TERMINATED) close() else core?.setPhase(phase)
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
                is AppException.Interrupted ->
                    "Stopped when the app went to the background. Try again."
                is AppException.ShutDown -> "The app is closing."
            }
    }
}
