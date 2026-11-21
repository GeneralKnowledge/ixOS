// Android's text-to-speech as Read Aloud's speaker: the counterpart of AVSpeechSynthesizer on the
// iPhone. It speaks with the voice and language chosen in the system's Text-to-speech settings.
package org.forgemobile.pocketlibrary.reader

import android.content.Context
import android.media.AudioAttributes
import android.os.Handler
import android.os.Looper
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener

class SystemSpeaker(context: Context) : Speaker {
    override var listener: Speaker.Listener? = null

    // TextToSpeech refuses longer text (4,000 characters).
    override val maxInputLength: Int = TextToSpeech.getMaxSpeechInputLength()

    private val main = Handler(Looper.getMainLooper())

    private enum class Engine { STARTING, READY, FAILED }

    private var engine = Engine.STARTING

    /** What to say once the engine has started. */
    private var waiting: Triple<String, String, Double>? = null

    private val tts: TextToSpeech = TextToSpeech(context.applicationContext) { status ->
        main.post { started(status == TextToSpeech.SUCCESS) }
    }

    init {
        // Spoken content, so the system treats it as media (volume keys, ducking).
        tts.setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                .build(),
        )
        // Called on the engine's own threads: hand everything to the main thread.
        tts.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
            override fun onStart(utteranceId: String) = Unit

            override fun onRangeStart(utteranceId: String, start: Int, end: Int, frame: Int) {
                main.post { listener?.onRange(utteranceId, start, end) }
            }

            override fun onDone(utteranceId: String) {
                main.post { listener?.onDone(utteranceId) }
            }

            @Deprecated("Deprecated in Java")
            override fun onError(utteranceId: String) {
                main.post { listener?.onError(utteranceId) }
            }

            override fun onError(utteranceId: String, errorCode: Int) {
                main.post { listener?.onError(utteranceId) }
            }

            // stop() interrupted it: the reader already moved on.
            override fun onStop(utteranceId: String, interrupted: Boolean) = Unit
        })
    }

    private fun started(ok: Boolean) {
        engine = if (ok) Engine.READY else Engine.FAILED
        val next = waiting ?: return
        waiting = null
        speak(next.first, next.second, next.third)
    }

    override fun speak(text: String, id: String, speed: Double) {
        when (engine) {
            Engine.STARTING -> waiting = Triple(text, id, speed)
            Engine.FAILED -> main.post { listener?.onError(id) }
            Engine.READY -> {
                // TextToSpeech's rate is linear, with 1 as the voice's normal pace.
                tts.setSpeechRate(speed.toFloat())
                if (tts.speak(text, TextToSpeech.QUEUE_FLUSH, null, id) != TextToSpeech.SUCCESS) {
                    main.post { listener?.onError(id) }
                }
            }
        }
    }

    override fun stop() {
        waiting = null
        if (engine == Engine.READY) tts.stop()
    }

    override fun shutdown() {
        waiting = null
        listener = null
        tts.shutdown()
    }
}

/** Makes Read Aloud's speaker. UI tests install a stand-in that records what would be said. */
object ReadAloudSpeaker {
    @Volatile
    var make: (Context) -> Speaker = { SystemSpeaker(it) }
}
