// Read Aloud on Android: the counterpart of the iPhone app's ReadAloud.swift. Plain Kotlin with
// no Android UI, so JVM tests drive it with a stand-in speaker; `SystemSpeaker` is the real one,
// on Android's TextToSpeech. Speech is presentation, not a rule, so it lives in the app: Rust has
// already turned the book into blocks, and the reader saves the place as the voice moves on.
package org.forgemobile.pocketlibrary.reader

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.pocket_library_core.ReaderBlock
import uniffi.pocket_library_core.ReaderBlockKind

/**
 * A text-to-speech engine, one utterance at a time. Calls to the listener come on the main
 * thread, as the reader's own calls do.
 */
interface Speaker {
    var listener: Listener?

    /** Speaks [text] at [speed] (1 is the voice's normal pace), replacing anything being said. */
    fun speak(text: String, id: String, speed: Double)

    /** Stops at once; the stopped utterance reports nothing more. */
    fun stop()

    /** Frees the engine. The speaker is not used again. */
    fun shutdown()

    interface Listener {
        /** Utterance [id] is about to say the characters from [start] to [end] of its text. */
        fun onRange(id: String, start: Int, end: Int)

        /** Utterance [id] was said to the end. */
        fun onDone(id: String)

        /** Utterance [id] couldn't be said: no voice, or the engine failed. */
        fun onError(id: String)
    }
}

/**
 * Reads a chapter aloud one block (paragraph, heading, quote...) per utterance, and reports the
 * block and word being said so the reader can follow and highlight them. Not thread-safe: call
 * it, and have the speaker call back, on the main thread.
 *
 * Android's TextToSpeech can't pause, so pausing stops the voice and remembers the word it had
 * reached; playing on starts that block again from that word.
 */
class ReadAloud(private val speaker: Speaker) : Speaker.Listener {
    enum class State { STOPPED, PLAYING, PAUSED }

    private val _state = MutableStateFlow(State.STOPPED)
    private val _block = MutableStateFlow<Int?>(null)
    private val _word = MutableStateFlow<IntRange?>(null)
    private val _speed = MutableStateFlow(1.0)
    private val _failed = MutableStateFlow(false)

    val state: StateFlow<State> = _state.asStateFlow()

    /** The block being read, as an index into the chapter's blocks. */
    val block: StateFlow<Int?> = _block.asStateFlow()

    /** The word being read, as character positions in the block's text (`speakableText`). */
    val word: StateFlow<IntRange?> = _word.asStateFlow()

    /** 0.75x to 3x, where 1x is the voice's normal pace. */
    val speed: StateFlow<Double> = _speed.asStateFlow()

    /** Set when the speaker couldn't say a block (reading has stopped); the screen clears it. */
    val failed: StateFlow<Boolean> = _failed.asStateFlow()

    /** Called with each block as reading reaches it, so the reader can save its place. */
    var onBlock: ((Int) -> Unit)? = null

    /**
     * Called when the last block of a chapter has been read, so the reader can load the next one
     * and call `play(0)`. Without it, reading stops at the end of the chapter.
     */
    var onChapterEnd: (() -> Unit)? = null

    private var texts: List<String> = emptyList()

    /** The utterance being said; callbacks for any other are stale. */
    private var current: String? = null
    private var utterances = 0L

    /** Where in its block the current utterance starts (after a pause, the word reached). */
    private var startOffset = 0

    init {
        speaker.listener = this
    }

    /**
     * The chapter to read. Stops reading the previous one; the state is kept, so a reader moving
     * on from the end of a chapter just calls `play(0)`.
     */
    fun load(blocks: List<ReaderBlock>) {
        texts = blocks.map(::speakableText)
        quiet()
        _block.value = null
        _word.value = null
    }

    /** Starts reading at block [index], or the next block with text. */
    fun play(index: Int) {
        _state.value = State.PLAYING
        speak(index, 0)
    }

    fun pause() {
        if (_state.value != State.PLAYING) return
        quiet()
        _state.value = State.PAUSED
    }

    fun resume() {
        if (_state.value != State.PAUSED) return
        _state.value = State.PLAYING
        speak(_block.value ?: 0, _word.value?.first ?: startOffset)
    }

    /** Plays from [index] when stopped, otherwise pauses or resumes. */
    fun toggle(index: Int) = when (_state.value) {
        State.STOPPED -> play(index)
        State.PLAYING -> pause()
        State.PAUSED -> resume()
    }

    /** The previous or next block with text; reading carries on from there. */
    fun skip(offset: Int) {
        val current = _block.value ?: return
        var target = current + offset
        while (target in texts.indices && texts[target].isEmpty()) target += offset
        if (target < 0) return
        _state.value = State.PLAYING
        speak(target, 0)
    }

    /** A new speed applies from the start of the block being read. */
    fun setSpeed(speed: Double) {
        _speed.value = speed
        val block = _block.value
        if (_state.value == State.PLAYING && block != null) speak(block, 0)
    }

    fun stop() {
        _state.value = State.STOPPED
        quiet()
        _block.value = null
        _word.value = null
    }

    /** Stops and frees the speaker: the reader is closing. */
    fun close() {
        stop()
        speaker.listener = null
        speaker.shutdown()
    }

    fun clearFailure() {
        _failed.value = false
    }

    private fun quiet() {
        current = null
        speaker.stop()
    }

    private fun speak(from: Int, offset: Int) {
        var index = maxOf(from, 0)
        while (index < texts.size && texts[index].isEmpty()) index += 1
        quiet()
        if (index >= texts.size) {
            _block.value = null
            _word.value = null
            onChapterEnd?.invoke() ?: stop()
            return
        }
        val text = texts[index]
        // A pause at the very end of a block plays on from its start.
        val start = if (offset in 1 until text.length) offset else 0
        val id = "readAloud.${++utterances}"
        current = id
        startOffset = start
        _block.value = index
        _word.value = null
        speaker.speak(text.substring(start), id, _speed.value)
        onBlock?.invoke(index)
    }

    override fun onRange(id: String, start: Int, end: Int) {
        if (id != current) return
        _word.value = (startOffset + start) until (startOffset + end)
    }

    override fun onDone(id: String) {
        if (id != current || _state.value != State.PLAYING) return
        speak((_block.value ?: -1) + 1, 0)
    }

    override fun onError(id: String) {
        if (id != current) return
        stop()
        _failed.value = true
    }

    companion object {
        /** The speeds offered, as on the iPhone. */
        val SPEEDS = listOf(0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0)

        /** "1.25×". */
        fun speedLabel(speed: Double): String {
            val text = if (speed % 1.0 == 0.0) speed.toInt().toString() else speed.toString().trimEnd('0')
            return "$text×"
        }

        /**
         * The text said for a block: exactly the text the reader shows, so word positions line
         * up with it. A list item's marker is shown beside it, not said; scene breaks and
         * pictures are skipped.
         */
        fun speakableText(block: ReaderBlock): String = when (block.kind) {
            is ReaderBlockKind.Separator, is ReaderBlockKind.Image -> ""
            else -> shownText(block)
        }

        /** The text the reader shows for a block: its spans, joined. */
        fun shownText(block: ReaderBlock): String = block.spans.joinToString("") { it.text }
    }
}
