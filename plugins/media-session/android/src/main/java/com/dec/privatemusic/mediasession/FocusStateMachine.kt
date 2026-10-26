package com.dec.privatemusic.mediasession

/**
 * Audio focus decisions, with no Android in them. The Android wrapper
 * ([AudioFocusController]) feeds events in and carries the returned effects out.
 *
 * The audio is produced by the Rust engine, so the machine never touches a
 * player: it asks the host to pause/resume/duck through [ActionEvent]s and tracks
 * *why* playback is paused, because only a pause caused by focus may be undone
 * by a focus gain. A pause by the user, a stop, or headphones being unplugged
 * must never restart by themselves.
 */
class FocusStateMachine {
    enum class Change { GAIN, LOSS, LOSS_TRANSIENT, LOSS_TRANSIENT_CAN_DUCK }
    enum class RequestResult { GRANTED, DELAYED, FAILED }

    sealed interface Effect {
        data object RequestFocus : Effect
        data object AbandonFocus : Effect
        data class Emit(val event: ActionEvent) : Effect
    }

    /** We hold focus (or a request is pending). */
    var hasFocus = false
        private set

    /** Paused because focus was lost (transiently, or a request was delayed). */
    var pausedByFocus = false
        private set

    var ducked = false
        private set

    private var playing = false
    private var awaitingPause = false
    private var gainBeforePause = false

    /** The app's own playback state changed (reported by the host). */
    fun onPlaybackState(kind: PlaybackKind): List<Effect> {
        val wasPlaying = playing
        playing = kind.isActive
        val effects = mutableListOf<Effect>()
        if (playing) {
            // The user (or the app) resumed by itself: that settles the question of "paused by focus".
            if (!awaitingPause) {
                pausedByFocus = false
                gainBeforePause = false
            }
            if (!hasFocus) effects += Effect.RequestFocus
        } else if (kind == PlaybackKind.STOPPED) {
            pausedByFocus = false
            awaitingPause = false
            gainBeforePause = false
            effects += endDuck()
            if (hasFocus) effects += release()
        } else if (pausedByFocus && awaitingPause) {
            awaitingPause = false
            if (gainBeforePause) {
                gainBeforePause = false
                pausedByFocus = false
                effects += Effect.Emit(ActionEvent.PLAY)
            }
        } else if (wasPlaying && !pausedByFocus) {
            // Paused by the user: keep focus for now, the service releases it when it idles.
            effects += endDuck()
        }
        return effects
    }

    /** Result of an `AudioManager.requestAudioFocus`. */
    fun onRequestResult(result: RequestResult): List<Effect> = when (result) {
        RequestResult.GRANTED -> {
            hasFocus = true
            emptyList()
        }
        // Another call/app owns the audio right now: do not play over it, but start by
        // ourselves once it is over.
        RequestResult.DELAYED -> {
            hasFocus = true
            pauseForFocus(resumeLater = true)
        }
        RequestResult.FAILED -> {
            hasFocus = false
            pauseForFocus(resumeLater = false)
        }
    }

    fun onFocusChange(change: Change): List<Effect> = when (change) {
        Change.GAIN -> {
            hasFocus = true
            val effects = mutableListOf<Effect>()
            effects += endDuck()
            if (pausedByFocus) {
                // Focus can return before Rust acknowledges Pause. Sending Play
                // now would race the pause and could leave the player stopped.
                if (awaitingPause) gainBeforePause = true
                else {
                    pausedByFocus = false
                    effects += Effect.Emit(ActionEvent.PLAY)
                }
            }
            effects
        }
        Change.LOSS -> {
            // Another app took over for good (a different music player): stay paused,
            // give the focus back, do not fight for it.
            hasFocus = false
            pausedByFocus = false
            awaitingPause = false
            gainBeforePause = false
            val effects = mutableListOf<Effect>()
            effects += endDuck()
            if (playing) effects += Effect.Emit(ActionEvent.PAUSE)
            effects
        }
        Change.LOSS_TRANSIENT -> {
            val effects = mutableListOf<Effect>()
            effects += endDuck()
            effects += pauseForFocus(resumeLater = true)
            effects
        }
        Change.LOSS_TRANSIENT_CAN_DUCK ->
            if (playing && !ducked) {
                ducked = true
                listOf(Effect.Emit(ActionEvent.duck(true)))
            } else {
                emptyList()
            }
    }

    /** Headphones unplugged / Bluetooth gone (`ACTION_AUDIO_BECOMING_NOISY`). */
    fun onBecomingNoisy(): List<Effect> {
        pausedByFocus = false
        awaitingPause = false
        gainBeforePause = false
        return if (playing) listOf(Effect.Emit(ActionEvent.PAUSE)) else emptyList()
    }

    /** The service is idling or ending: give focus back unless we are waiting for it to return. */
    fun onIdle(force: Boolean): List<Effect> {
        if (!hasFocus && !pausedByFocus) return emptyList()
        if (pausedByFocus && !force) return emptyList()
        pausedByFocus = false
        awaitingPause = false
        gainBeforePause = false
        return endDuck() + release()
    }

    private fun release(): List<Effect> {
        hasFocus = false
        return listOf(Effect.AbandonFocus)
    }

    private fun endDuck(): List<Effect> {
        if (!ducked) return emptyList()
        ducked = false
        return listOf(Effect.Emit(ActionEvent.duck(false)))
    }

    private fun pauseForFocus(resumeLater: Boolean): List<Effect> {
        if (!playing) return emptyList()
        pausedByFocus = resumeLater
        awaitingPause = resumeLater
        gainBeforePause = false
        return listOf(Effect.Emit(ActionEvent.PAUSE))
    }
}
