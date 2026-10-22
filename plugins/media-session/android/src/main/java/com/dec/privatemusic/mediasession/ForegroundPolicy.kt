package com.dec.privatemusic.mediasession

/**
 * When the playback service must be a foreground service, when it may let go and
 * when it must stop. Pure: time is passed in, the service owns the timer.
 *
 * Timeline after a pause: FOREGROUND for [Config.pauseGraceMs] (a call, a short
 * interruption; the process keeps its priority so a focus gain can resume), then
 * DETACHED (notification stays but is swipeable and no longer ongoing), then
 * STOPPED after [Config.idleStopMs]. A swipe while paused stops at once.
 */
class ForegroundPolicy(
    private val config: Config = Config(),
    startedAtMs: Long = 0,
) {
    data class Config(
        val pauseGraceMs: Long = 30_000,
        val idleStopMs: Long = 10 * 60_000,
        /** Time to ride out a track change that passes through "stopped". */
        val stoppedGraceMs: Long = 5_000,
    )

    enum class Mode { FOREGROUND, DETACHED, STOPPED }

    /** [recheckAtMs]: when to ask again (null: nothing changes without an event). */
    data class Decision(val mode: Mode, val recheckAtMs: Long?)

    private var kind = PlaybackKind.STOPPED
    private var since = startedAtMs
    private var dismissed = false
    private var ended = false

    fun onPlayback(newKind: PlaybackKind, nowMs: Long): Decision {
        if (newKind != kind) {
            kind = newKind
            since = nowMs
            dismissed = false
        }
        return decide(nowMs)
    }

    fun decide(nowMs: Long): Decision {
        if (ended) return Decision(Mode.STOPPED, null)
        val elapsed = nowMs - since
        return when (kind) {
            PlaybackKind.PLAYING, PlaybackKind.BUFFERING -> Decision(Mode.FOREGROUND, null)
            PlaybackKind.PAUSED -> when {
                dismissed || elapsed >= config.idleStopMs -> Decision(Mode.STOPPED, null)
                elapsed >= config.pauseGraceMs -> Decision(Mode.DETACHED, since + config.idleStopMs)
                else -> Decision(Mode.FOREGROUND, since + config.pauseGraceMs)
            }
            PlaybackKind.STOPPED ->
                if (dismissed || elapsed >= config.stoppedGraceMs) {
                    Decision(Mode.STOPPED, null)
                } else {
                    Decision(Mode.FOREGROUND, since + config.stoppedGraceMs)
                }
        }
    }

    /** The user swiped the notification away. Only meaningful (and only possible) when not playing. */
    fun onNotificationDismissed(nowMs: Long): Decision {
        if (!kind.isActive) dismissed = true
        return decide(nowMs)
    }

    /** The app was removed from recents: keep playing, otherwise leave. */
    fun onTaskRemoved(nowMs: Long): Decision {
        if (!kind.isActive) ended = true
        return decide(nowMs)
    }

    /** `Service.onTimeout` (API 34/35): the system is about to stop us; stop cleanly. */
    fun onTimeout(): Decision {
        ended = true
        return Decision(Mode.STOPPED, null)
    }

    /** Explicit end (the `stop` command). */
    fun end(): Decision = onTimeout()
}
