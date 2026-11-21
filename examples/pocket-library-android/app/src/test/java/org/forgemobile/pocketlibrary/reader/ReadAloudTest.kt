package org.forgemobile.pocketlibrary.reader

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.pocket_library_core.ReaderBlock
import uniffi.pocket_library_core.ReaderBlockKind
import uniffi.pocket_library_core.ReaderSpan

/** Read Aloud with a stand-in speaker: what is said, in what order, and when it stops. */
class ReadAloudTest {
    private class StandIn : Speaker {
        override var listener: Speaker.Listener? = null
        val said = mutableListOf<String>()
        var lastId = ""
        var stops = 0
        var shutDown = false
        override var maxInputLength = Int.MAX_VALUE

        override fun speak(text: String, id: String, speed: Double) {
            said += text
            lastId = id
        }

        override fun stop() {
            stops += 1
        }

        override fun shutdown() {
            shutDown = true
        }

        fun finish() = listener!!.onDone(lastId)
    }

    private fun span(text: String, bold: Boolean = false) = ReaderSpan(text, bold, false, false, null)

    private fun block(kind: ReaderBlockKind, vararg spans: ReaderSpan) = ReaderBlock("", kind, spans.toList())

    private val chapter = listOf(
        block(ReaderBlockKind.Heading(1u), span("The Storm")),
        block(ReaderBlockKind.Paragraph, span("It was a "), span("dark", bold = true), span(" night.")),
        block(ReaderBlockKind.Separator),
        block(ReaderBlockKind.Paragraph, span("Morning came.")),
    )

    /** PL-9's rule, as the iPhone's ReadAloudTests.testSpokenTextIsTheShownText. */
    @Test
    fun spokenTextIsTheShownText() {
        val paragraph = chapter[1]
        assertEquals("It was a dark night.", ReadAloud.speakableText(paragraph))
        assertEquals(ReadAloud.shownText(paragraph), ReadAloud.speakableText(paragraph))
        // The marker is shown beside the item, not said.
        assertEquals("Three", ReadAloud.speakableText(block(ReaderBlockKind.ListItem("3."), span("Three"))))
        assertEquals("", ReadAloud.speakableText(block(ReaderBlockKind.Separator)))
        assertEquals("", ReadAloud.speakableText(block(ReaderBlockKind.Image("/x.png", "Map"))))
    }

    @Test
    fun readsBlockByBlockSkippingBreaksAndStopsAtTheEnd() {
        val speaker = StandIn()
        val readAloud = ReadAloud(speaker)
        val reached = mutableListOf<Int>()
        readAloud.onBlock = { reached += it }
        readAloud.load(chapter)
        readAloud.play(0)
        assertEquals(ReadAloud.State.PLAYING, readAloud.state.value)
        speaker.finish()
        speaker.finish()
        assertEquals(listOf("The Storm", "It was a dark night.", "Morning came."), speaker.said)
        assertEquals(listOf(0, 1, 3), reached)
        assertEquals(3, readAloud.block.value)
        // No next chapter to go to: reading stops.
        speaker.finish()
        assertEquals(ReadAloud.State.STOPPED, readAloud.state.value)
        assertNull(readAloud.block.value)
    }

    @Test
    fun theChapterEndHandsOverToTheReader() {
        val speaker = StandIn()
        val readAloud = ReadAloud(speaker)
        var ended = 0
        readAloud.onChapterEnd = { ended += 1 }
        readAloud.load(chapter)
        readAloud.play(3)
        speaker.finish()
        assertEquals(1, ended)
        // Still playing: the reader loads the next chapter and plays it.
        assertEquals(ReadAloud.State.PLAYING, readAloud.state.value)
    }

    @Test
    fun pauseKeepsTheWordAndResumeSaysTheRestOfTheBlock() {
        val speaker = StandIn()
        val readAloud = ReadAloud(speaker)
        readAloud.load(chapter)
        readAloud.play(1)
        // "It was a dark night.": the voice reaches "dark".
        speaker.listener!!.onRange(speaker.lastId, 9, 13)
        assertEquals(9 until 13, readAloud.word.value)
        readAloud.pause()
        assertEquals(ReadAloud.State.PAUSED, readAloud.state.value)
        readAloud.resume()
        assertEquals("dark night.", speaker.said.last())
        // Word positions stay in the whole block's text.
        speaker.listener!!.onRange(speaker.lastId, 5, 11)
        assertEquals(14 until 20, readAloud.word.value)
    }

    @Test
    fun staleCallbacksAreIgnored() {
        val speaker = StandIn()
        val readAloud = ReadAloud(speaker)
        readAloud.load(chapter)
        readAloud.play(0)
        val first = speaker.lastId
        readAloud.skip(1)
        assertEquals("It was a dark night.", speaker.said.last())
        // The skipped utterance finishing doesn't move reading on.
        speaker.listener!!.onDone(first)
        assertEquals(1, readAloud.block.value)
        // Skipping on passes the scene break.
        readAloud.skip(1)
        assertEquals(3, readAloud.block.value)
    }

    @Test
    fun aFailedUtteranceStopsAndSaysSo() {
        val speaker = StandIn()
        val readAloud = ReadAloud(speaker)
        readAloud.load(chapter)
        readAloud.play(0)
        speaker.listener!!.onError(speaker.lastId)
        assertEquals(ReadAloud.State.STOPPED, readAloud.state.value)
        assertTrue(readAloud.failed.value)
        readAloud.clearFailure()
        assertFalse(readAloud.failed.value)
        readAloud.close()
        assertTrue(speaker.shutDown)
    }

    @Test
    fun speedLabels() {
        assertEquals(listOf("0.75×", "1×", "1.25×", "1.5×", "1.75×", "2×", "2.5×", "3×"), ReadAloud.SPEEDS.map(ReadAloud::speedLabel))
    }

    /** TextToSpeech takes at most 4,000 characters: a longer block is said in pieces, as one block. */
    @Test
    fun aLongBlockIsSaidInPiecesAtSentences() {
        val speaker = StandIn().apply { maxInputLength = 20 }
        val reader = ReadAloud(speaker)
        val blocks = mutableListOf<Int>()
        reader.onBlock = { blocks += it }
        val long = "One two three. Four five six seven eight nine."
        reader.load(listOf(block(ReaderBlockKind.Paragraph, span(long)), block(ReaderBlockKind.Paragraph, span("End."))))
        reader.play(0)
        speaker.listener!!.onRange(speaker.lastId, 0, 3)
        assertEquals(0 until 3, reader.word.value)
        speaker.finish()
        // Word positions stay positions in the whole block.
        speaker.listener!!.onRange(speaker.lastId, 0, 4)
        assertEquals(15 until 19, reader.word.value)
        speaker.finish()
        speaker.finish()
        assertEquals(listOf("One two three. ", "Four five six seven ", "eight nine.", "End."), speaker.said)
        assertEquals(long, speaker.said.take(3).joinToString(""))
        assertEquals(listOf(0, 1), blocks)
    }

    @Test
    fun aPieceEndsAtASentenceThenASpaceThenTheLimit() {
        assertEquals(5, ReadAloud.pieceEnd("short", 0, 10))
        assertEquals(15, ReadAloud.pieceEnd("One two three. Four five six", 0, 20))
        assertEquals(7, ReadAloud.pieceEnd("onetwo three", 0, 9))
        assertEquals(4, ReadAloud.pieceEnd("abcdefgh", 0, 4))
        // Never between the two halves of a character outside the basic plane.
        assertEquals(3, ReadAloud.pieceEnd("abc\uD83D\uDE00defgh", 0, 4))
    }
}
