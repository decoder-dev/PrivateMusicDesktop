package com.dec.privatemusic.mediasession

/** The buttons of the playback notification and what they do. */
enum class ButtonId(val intentAction: String) {
    PREVIOUS("com.dec.privatemusic.mediasession.PREVIOUS"),
    PLAY("com.dec.privatemusic.mediasession.PLAY"),
    PAUSE("com.dec.privatemusic.mediasession.PAUSE"),
    NEXT("com.dec.privatemusic.mediasession.NEXT"),
    STOP("com.dec.privatemusic.mediasession.STOP"),
    LIKE("com.dec.privatemusic.mediasession.LIKE");

    fun toEvent(): ActionEvent = when (this) {
        PREVIOUS -> ActionEvent.PREVIOUS
        PLAY -> ActionEvent.PLAY
        PAUSE -> ActionEvent.PAUSE
        NEXT -> ActionEvent.NEXT
        STOP -> ActionEvent.STOP
        LIKE -> ActionEvent.LIKE
    }

    companion object {
        fun fromIntentAction(action: String?): ButtonId? = entries.firstOrNull { it.intentAction == action }
    }
}

object NotificationLayout {
    /**
     * Previous / play-or-pause / next, in that order, then Like when the track can be added to My music
     * (`liked` is null for tracks that cannot: local files, nothing playing). Nothing to show when stopped.
     */
    fun buttons(kind: PlaybackKind, canNext: Boolean = true, liked: Boolean? = null): List<ButtonId> {
        val transport = when (kind) {
            PlaybackKind.PLAYING, PlaybackKind.BUFFERING -> listOf(ButtonId.PREVIOUS, ButtonId.PAUSE, ButtonId.NEXT)
            PlaybackKind.PAUSED -> listOf(ButtonId.PREVIOUS, ButtonId.PLAY, ButtonId.NEXT)
            PlaybackKind.STOPPED -> emptyList()
        }.filter { it != ButtonId.NEXT || canNext }
        return if (transport.isNotEmpty() && liked != null) transport + ButtonId.LIKE else transport
    }

    /** The transport buttons stay visible in the collapsed notification; Like shows when it is expanded. */
    fun compactIndices(kind: PlaybackKind, canNext: Boolean = true, liked: Boolean? = null): IntArray =
        buttons(kind, canNext, liked).withIndex().filter { it.value != ButtonId.LIKE }.map { it.index }.toIntArray()

    /** Ongoing (not swipeable) only while the user wants sound. */
    fun isOngoing(kind: PlaybackKind): Boolean = kind.isActive
}
