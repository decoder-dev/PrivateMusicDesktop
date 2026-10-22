package com.dec.privatemusic.mediasession

import android.view.KeyEvent

/**
 * Hardware/headset keys that map straight onto an action. KEYCODE_HEADSETHOOK is
 * deliberately absent: its single/double/triple click logic is done by
 * `MediaSessionCompat.Callback.onMediaButtonEvent` (play-pause / next / previous).
 */
object MediaKeyMapper {
    fun actionFor(keyCode: Int): ActionEvent? = when (keyCode) {
        KeyEvent.KEYCODE_MEDIA_PLAY -> ActionEvent.PLAY
        KeyEvent.KEYCODE_MEDIA_PAUSE -> ActionEvent.PAUSE
        KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE -> ActionEvent.TOGGLE
        KeyEvent.KEYCODE_MEDIA_NEXT -> ActionEvent.NEXT
        KeyEvent.KEYCODE_MEDIA_PREVIOUS -> ActionEvent.PREVIOUS
        KeyEvent.KEYCODE_MEDIA_STOP -> ActionEvent.STOP
        KeyEvent.KEYCODE_MEDIA_FAST_FORWARD -> ActionEvent.seekBy(SEEK_STEP_SECONDS)
        KeyEvent.KEYCODE_MEDIA_REWIND -> ActionEvent.seekBy(-SEEK_STEP_SECONDS)
        else -> null
    }

    const val SEEK_STEP_SECONDS = 10.0
}
