package com.dec.privatemusic.mediasession

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.util.Log

/**
 * Android side of [FocusStateMachine]: owns the `AudioFocusRequest` and carries the
 * machine's effects out. Main thread only (the listener is bound to the main looper).
 */
class AudioFocusController(context: Context, private val emit: (ActionEvent) -> Unit) {
    private val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val handler = Handler(Looper.getMainLooper())
    private val machine = FocusStateMachine()
    private var request: AudioFocusRequest? = null

    private val listener = AudioManager.OnAudioFocusChangeListener { change ->
        val mapped = when (change) {
            AudioManager.AUDIOFOCUS_GAIN,
            AudioManager.AUDIOFOCUS_GAIN_TRANSIENT,
            AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK,
            AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE -> FocusStateMachine.Change.GAIN
            AudioManager.AUDIOFOCUS_LOSS -> FocusStateMachine.Change.LOSS
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> FocusStateMachine.Change.LOSS_TRANSIENT
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> FocusStateMachine.Change.LOSS_TRANSIENT_CAN_DUCK
            else -> null
        }
        if (mapped != null) run(machine.onFocusChange(mapped))
    }

    fun onPlaybackState(kind: PlaybackKind) = run(machine.onPlaybackState(kind))

    /** The `requestAudioFocus` command: take focus now if we do not hold it. */
    fun requestNow() {
        if (!machine.hasFocus) request()
    }

    /** The `abandonAudioFocus` command and the end of the service. */
    fun abandon() = run(machine.onIdle(force = true))

    /** Service is idle but may still be resumed from a transient loss: keep waiting for the gain. */
    fun onIdle() = run(machine.onIdle(force = false))

    fun onBecomingNoisy() = run(machine.onBecomingNoisy())

    private fun run(effects: List<FocusStateMachine.Effect>) {
        for (effect in effects) {
            when (effect) {
                FocusStateMachine.Effect.RequestFocus -> request()
                FocusStateMachine.Effect.AbandonFocus -> abandonSystem()
                is FocusStateMachine.Effect.Emit -> emit(effect.event)
            }
        }
    }

    private fun request() {
        val attributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()
        val focusRequest = request ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attributes)
            .setWillPauseWhenDucked(false)
            .setAcceptsDelayedFocusGain(true)
            .setOnAudioFocusChangeListener(listener, handler)
            .build()
            .also { request = it }
        val result = try {
            audioManager.requestAudioFocus(focusRequest)
        } catch (e: RuntimeException) {
            Log.w(TAG, "audio focus request failed: ${e.javaClass.simpleName}")
            AudioManager.AUDIOFOCUS_REQUEST_FAILED
        }
        val mapped = when (result) {
            AudioManager.AUDIOFOCUS_REQUEST_GRANTED -> FocusStateMachine.RequestResult.GRANTED
            AudioManager.AUDIOFOCUS_REQUEST_DELAYED -> FocusStateMachine.RequestResult.DELAYED
            else -> FocusStateMachine.RequestResult.FAILED
        }
        run(machine.onRequestResult(mapped))
    }

    private fun abandonSystem() {
        request?.let {
            try {
                audioManager.abandonAudioFocusRequest(it)
            } catch (e: RuntimeException) {
                Log.w(TAG, "abandoning audio focus failed: ${e.javaClass.simpleName}")
            }
        }
    }

    private companion object {
        const val TAG = "MediaSession"
    }
}
