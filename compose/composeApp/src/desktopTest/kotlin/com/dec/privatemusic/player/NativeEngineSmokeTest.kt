package com.dec.privatemusic.player

import kotlin.test.Test
import kotlin.test.assertTrue
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlinx.coroutines.runBlocking

/** Verifies that JNA can load the Rust resource without a developer JNA_LIBRARY_PATH. */
class NativeEngineSmokeTest {
    @Test
    fun packagedRustLibraryLoadsFromClasspath() = runBlocking {
        val previous = System.getProperty("pm.compose.demo")
        try {
            System.setProperty("pm.compose.demo", "0")
            val api = platformPlayerApi()
            try {
                assertTrue(api.state().status.isNotBlank())
                platformCatalogApi().downloads()
                api.setVolume(0.75f)
                api.playTracks(demoTracks, 1)
                api.setShuffle(true)
                api.setRepeat("one")
                assertTrue(api.state().shuffle)
                assertEquals("one", api.state().repeat)
                api.setShuffle(false)
                api.setRepeat("off")

                val extraTrack = demoTracks.first().copy(track_id = 999, title = "Очередь")
                api.playNext(extraTrack)
                val withNext = api.queueSnapshot()
                assertEquals(extraTrack, withNext.tracks[withNext.playbackIndices[2]])
                api.moveUpcoming(withNext.playbackIndices[2], withNext.playbackIndices[3])
                val reordered = api.queueSnapshot()
                assertEquals(extraTrack, reordered.tracks[reordered.playbackIndices[3]])
                api.removeFromQueue(reordered.playbackIndices[3])
                api.playLast(extraTrack)
                assertEquals(extraTrack, api.queueSnapshot().tracks.last())
                api.clearUpcoming()
                assertEquals(QueueEvent(demoTracks.take(2), 1, listOf(0, 1)), api.queueSnapshot())

                val gains = List(10) { it.toFloat() - 4.5f }
                api.setEqualizer(gains)
                assertEquals(gains, api.equalizer())
                api.setEqualizer(null)
                assertNull(api.equalizer())

                api.setSleepTimer(null, true)
                assertTrue(api.sleepTimer().end_of_track)
                api.setSleepTimer(30, false)
                assertTrue(api.sleepTimer().active)
                api.setSleepTimer(null, false)
                assertEquals(SleepTimer(), api.sleepTimer())
            } finally {
                api.close()
            }
        } finally {
            if (previous == null) System.clearProperty("pm.compose.demo")
            else System.setProperty("pm.compose.demo", previous)
        }
    }
}
