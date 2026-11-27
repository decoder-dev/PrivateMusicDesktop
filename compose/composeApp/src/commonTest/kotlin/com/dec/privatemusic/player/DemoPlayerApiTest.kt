package com.dec.privatemusic.player

import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals

class DemoPlayerApiTest {
    @Test
    fun queueTransportAndSeekShareOneState() = runTest {
        val api = DemoPlayerApi()
        api.playTracks(demoTracks, 1)
        assertEquals("Engel", api.state().track?.title)
        assertEquals(1, api.state().queue_index)
        api.seek(42.0)
        assertEquals(42.0, api.state().position)
        api.toggle()
        assertEquals("paused", api.state().status)
        api.next()
        assertEquals("The Cycle of Songs", api.state().track?.title)
        assertEquals(0.0, api.state().position)
    }

    @Test
    fun seekIsClampedToTrackBounds() = runTest {
        val api = DemoPlayerApi()
        api.playTracks(demoTracks, 0)
        api.seek(-10.0)
        assertEquals(0.0, api.state().position)
        api.seek(10_000.0)
        assertEquals(demoTracks[0].duration.toDouble(), api.state().position)
    }
}
