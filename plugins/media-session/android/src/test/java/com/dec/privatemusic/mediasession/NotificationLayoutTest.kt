package com.dec.privatemusic.mediasession

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationLayoutTest {
    @Test
    fun playingShowsPreviousPauseNext() {
        for (kind in listOf(PlaybackKind.PLAYING, PlaybackKind.BUFFERING)) {
            assertEquals(listOf(ButtonId.PREVIOUS, ButtonId.PAUSE, ButtonId.NEXT), NotificationLayout.buttons(kind))
        }
    }

    @Test
    fun pausedShowsPreviousPlayNext() {
        assertEquals(listOf(ButtonId.PREVIOUS, ButtonId.PLAY, ButtonId.NEXT), NotificationLayout.buttons(PlaybackKind.PAUSED))
    }

    @Test
    fun stoppedShowsNoButtons() {
        assertTrue(NotificationLayout.buttons(PlaybackKind.STOPPED).isEmpty())
        assertEquals(0, NotificationLayout.compactIndices(PlaybackKind.STOPPED).size)
    }

    @Test
    fun allButtonsAreShownInTheCompactView() {
        assertArrayEquals(intArrayOf(0, 1, 2), NotificationLayout.compactIndices(PlaybackKind.PLAYING))
    }

    @Test
    fun onlyActivePlaybackIsOngoing() {
        assertTrue(NotificationLayout.isOngoing(PlaybackKind.PLAYING))
        assertTrue(NotificationLayout.isOngoing(PlaybackKind.BUFFERING))
        assertFalse(NotificationLayout.isOngoing(PlaybackKind.PAUSED))
        assertFalse(NotificationLayout.isOngoing(PlaybackKind.STOPPED))
    }

    @Test
    fun buttonsMapToActionsAndBack() {
        assertEquals(ActionEvent.PLAY, ButtonId.PLAY.toEvent())
        assertEquals(ActionEvent.PAUSE, ButtonId.PAUSE.toEvent())
        assertEquals(ActionEvent.NEXT, ButtonId.NEXT.toEvent())
        assertEquals(ActionEvent.PREVIOUS, ButtonId.PREVIOUS.toEvent())
        assertEquals(ActionEvent.STOP, ButtonId.STOP.toEvent())
        for (button in ButtonId.entries) assertEquals(button, ButtonId.fromIntentAction(button.intentAction))
        assertNull(ButtonId.fromIntentAction("android.intent.action.VIEW"))
        assertNull(ButtonId.fromIntentAction(null))
    }

    @Test
    fun intentActionsAreUnique() {
        assertEquals(ButtonId.entries.size, ButtonId.entries.map { it.intentAction }.toSet().size)
    }
}
