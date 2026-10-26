package com.dec.privatemusic.mediasession

/** The buttons of the playback notification and what they do. */
enum class ButtonId(val intentAction: String) {
    PREVIOUS("com.dec.privatemusic.mediasession.PREVIOUS"),
    PLAY("com.dec.privatemusic.mediasession.PLAY"),
    PAUSE("com.dec.privatemusic.mediasession.PAUSE"),
    NEXT("com.dec.privatemusic.mediasession.NEXT"),
    STOP("com.dec.privatemusic.mediasession.STOP");

    fun toEvent(): ActionEvent = when (this) {
        PREVIOUS -> ActionEvent.PREVIOUS
        PLAY -> ActionEvent.PLAY
        PAUSE -> ActionEvent.PAUSE
        NEXT -> ActionEvent.NEXT
        STOP -> ActionEvent.STOP
    }

    companion object {
        fun fromIntentAction(action: String?): ButtonId? = entries.firstOrNull { it.intentAction == action }
    }
}

object NotificationLayout {
    /** Previous / play-or-pause / next, in that order. Nothing to show when stopped. */
    fun buttons(kind: PlaybackKind, canNext: Boolean = true): List<ButtonId> = when (kind) {
        PlaybackKind.PLAYING, PlaybackKind.BUFFERING -> listOf(ButtonId.PREVIOUS, ButtonId.PAUSE, ButtonId.NEXT)
        PlaybackKind.PAUSED -> listOf(ButtonId.PREVIOUS, ButtonId.PLAY, ButtonId.NEXT)
        PlaybackKind.STOPPED -> emptyList()
    }.filter { it != ButtonId.NEXT || canNext }

    /** All three buttons stay visible in the collapsed notification. */
    fun compactIndices(kind: PlaybackKind, canNext: Boolean = true): IntArray = IntArray(buttons(kind, canNext).size) { it }

    /** Ongoing (not swipeable) only while the user wants sound. */
    fun isOngoing(kind: PlaybackKind): Boolean = kind.isActive
}
