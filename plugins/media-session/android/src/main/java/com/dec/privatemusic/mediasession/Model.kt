package com.dec.privatemusic.mediasession

/** Playback state as sent by Rust (`PlaybackKind`, snake_case on the wire). */
enum class PlaybackKind(val wire: String) {
    PLAYING("playing"),
    PAUSED("paused"),
    BUFFERING("buffering"),
    STOPPED("stopped");

    /** The user wants sound: the process must stay alive. */
    val isActive: Boolean get() = this == PLAYING || this == BUFFERING

    companion object {
        fun fromWire(value: String?): PlaybackKind =
            entries.firstOrNull { it.wire == value } ?: STOPPED
    }
}

data class TrackInfo(
    val title: String,
    val artist: String,
    val album: String?,
    val artworkUrl: String?,
    val durationMs: Long,
)

/** Playback state at the instant [PlaybackSnapshot.receivedAtMs]. */
data class PlaybackInfo(
    val kind: PlaybackKind,
    val positionMs: Long,
    val durationMs: Long,
    val rate: Float,
    val canNext: Boolean = true,
)

/** What the host last told us, with the (monotonic) time it arrived. */
data class PlaybackSnapshot(val info: PlaybackInfo, val receivedAtMs: Long) {
    /** The position now: the system scrubber extrapolates, so must we. */
    fun positionAt(nowMs: Long): Long = PlaybackStateMapper.extrapolate(info, receivedAtMs, nowMs)
}

/**
 * A request from the system to the app. Wire format (JSON object sent through the
 * Tauri channel) is `{"action": name}` or `{"action": name, "value": number|bool}`;
 * it must match `MediaAction` in `lib.rs`.
 */
data class ActionEvent(val name: String, val value: Any? = null, val source: String? = null) {
    companion object {
        val PLAY = ActionEvent("play")
        val PAUSE = ActionEvent("pause")
        val TOGGLE = ActionEvent("toggle")
        val NEXT = ActionEvent("next")
        val PREVIOUS = ActionEvent("previous")
        val STOP = ActionEvent("stop")
        fun seekTo(seconds: Double) = ActionEvent("seek_to", seconds)
        fun seekBy(seconds: Double) = ActionEvent("seek_by", seconds)
        fun duck(on: Boolean) = ActionEvent("duck", on)
    }
}
