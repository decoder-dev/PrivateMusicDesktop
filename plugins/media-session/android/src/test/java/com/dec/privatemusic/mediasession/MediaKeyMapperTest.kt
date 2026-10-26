package com.dec.privatemusic.mediasession

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MediaKeyMapperTest {
    @Test
    fun transportKeysMapToActions() {
        assertEquals(ActionEvent.PLAY, MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_PLAY))
        assertEquals(ActionEvent.PAUSE, MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_PAUSE))
        assertEquals(ActionEvent.TOGGLE, MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE))
        assertEquals(ActionEvent.NEXT, MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_NEXT))
        assertEquals(ActionEvent.PREVIOUS, MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_PREVIOUS))
        assertEquals(ActionEvent.STOP, MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_STOP))
        assertEquals(ActionEvent.seekBy(10.0), MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_FAST_FORWARD))
        assertEquals(ActionEvent.seekBy(-10.0), MediaKeyMapper.actionFor(KeyEvent.KEYCODE_MEDIA_REWIND))
    }

    @Test
    fun theHeadsetHookIsLeftToTheClickCounter() {
        assertNull(MediaKeyMapper.actionFor(KeyEvent.KEYCODE_HEADSETHOOK))
        assertNull(MediaKeyMapper.actionFor(KeyEvent.KEYCODE_VOLUME_UP))
        assertNull(MediaKeyMapper.actionFor(KeyEvent.KEYCODE_VOLUME_DOWN))
        assertNull(MediaKeyMapper.actionFor(KeyEvent.KEYCODE_VOLUME_MUTE))
        assertNull(MediaKeyMapper.actionFor(KeyEvent.KEYCODE_A))
    }
}
