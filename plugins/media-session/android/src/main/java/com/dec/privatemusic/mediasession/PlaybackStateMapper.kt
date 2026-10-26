package com.dec.privatemusic.mediasession

import android.support.v4.media.session.PlaybackStateCompat

/** Playback info → what `PlaybackStateCompat` needs. No Android calls, only constants. */
object PlaybackStateMapper {
    data class Mapped(val state: Int, val positionMs: Long, val speed: Float, val actions: Long)

    private const val BASE_ACTIONS: Long =
        PlaybackStateCompat.ACTION_PLAY or
            PlaybackStateCompat.ACTION_PAUSE or
            PlaybackStateCompat.ACTION_PLAY_PAUSE or
            PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or
            PlaybackStateCompat.ACTION_STOP

    /**
     * [positionMs] is the position now (extrapolated by the caller if the state is
     * older than the call). While playing, the system advances the scrubber from
     * position + speed on its own, so no per-second updates are needed.
     */
    fun map(info: PlaybackInfo, hasTrack: Boolean, positionMs: Long = info.positionMs): Mapped {
        val duration = info.durationMs
        val position = if (duration > 0) positionMs.coerceIn(0, duration) else positionMs.coerceAtLeast(0)
        // The scrubber is only useful when the length is known.
        val seek = if (duration > 0) PlaybackStateCompat.ACTION_SEEK_TO else 0L
        val actions = BASE_ACTIONS or seek or if (info.canNext) PlaybackStateCompat.ACTION_SKIP_TO_NEXT else 0L
        return when (info.kind) {
            PlaybackKind.PLAYING ->
                Mapped(PlaybackStateCompat.STATE_PLAYING, position, info.rate, actions)
            PlaybackKind.PAUSED ->
                Mapped(PlaybackStateCompat.STATE_PAUSED, position, 0f, actions)
            // Speed 0 while buffering: the scrubber must not run ahead of the sound.
            PlaybackKind.BUFFERING ->
                Mapped(PlaybackStateCompat.STATE_BUFFERING, position, 0f, actions)
            PlaybackKind.STOPPED ->
                Mapped(
                    PlaybackStateCompat.STATE_STOPPED,
                    0,
                    0f,
                    if (hasTrack) PlaybackStateCompat.ACTION_PLAY or PlaybackStateCompat.ACTION_PLAY_PAUSE else 0L,
                )
        }
    }

    /** Where the playhead is [nowMs] given the state received at [receivedAtMs]. */
    fun extrapolate(info: PlaybackInfo, receivedAtMs: Long, nowMs: Long): Long {
        if (info.kind != PlaybackKind.PLAYING) return info.positionMs
        val elapsed = (nowMs - receivedAtMs).coerceAtLeast(0)
        val position = info.positionMs + (elapsed * info.rate).toLong()
        return if (info.durationMs > 0) position.coerceAtMost(info.durationMs) else position
    }
}
