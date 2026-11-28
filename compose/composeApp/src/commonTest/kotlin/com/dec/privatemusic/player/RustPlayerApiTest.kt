package com.dec.privatemusic.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.withTimeout

class RustPlayerApiTest {
    @Test
    fun nativeEventsAreBroadcastToAllCollectors() = runBlocking {
        val transport = FakeTransport()
        val api = RustPlayerApi(transport)
        val first = async { api.events().first() }
        val second = async { api.events().first() }
        delay(50)
        transport.gate.unlock()
        withTimeout(2_000) {
            assertEquals("""{"status":"playing"}""", first.await())
            assertEquals("""{"status":"playing"}""", second.await())
        }
        api.close()
        assertEquals(1, transport.eventsSent)
    }

    @Test
    fun equalizerGainsAreEncodedAsJsonNumbers() = runBlocking {
        val transport = FakeTransport()
        val api = RustPlayerApi(transport)
        api.setEqualizer(listOf(0.25f, -1.5f))
        assertTrue(transport.lastCommand.contains("0.25"))
        assertTrue(transport.lastCommand.contains("-1.5"))
        api.close()
    }

    @Test
    fun stateSendsExactlyOneNativeCommand() = runBlocking {
        val transport = FakeTransport()
        val api = RustPlayerApi(transport)
        api.state()
        assertEquals(1, transport.commandCount)
        assertEquals("""{"command":"player_state"}""", transport.lastCommand)
        api.close()
    }

    @Test
    fun playTracksEncodesRustRequiredDefaults() = runBlocking {
        val transport = FakeTransport()
        val api = RustPlayerApi(transport)
        api.playTracks(listOf(demoTracks.first()), 0)
        assertTrue(transport.lastCommand.contains("\"is_hq\":false"))
        assertTrue(transport.lastCommand.contains("\"artists\":[]"))
        assertTrue(transport.lastCommand.contains("\"stream_url\":null"))
        api.close()
    }

    @Test
    fun catalogAndPlaybackAcceptNativeArraysPrimitivesAndNull() = runBlocking {
        val api = RustPlayerApi(FakeTransport { raw ->
            when {
                raw.contains("\"downloads\"") -> "[]"
                raw.contains("\"output_devices\"") -> """["Динамики"]"""
                raw.contains("\"remove_download\"") -> "false"
                else -> "null"
            }
        })
        try {
            api.toggle()
            assertEquals(emptyList(), api.downloads())
            assertEquals(listOf("Динамики"), api.outputDevices())
            assertFalse(api.removeDownload("1_101"))
            assertNull(api.lyrics(demoTracks.first()))
        } finally {
            api.close()
        }
    }

    @Test
    fun nativeErrorsReachCatalogAndPlaybackCallers() = runBlocking {
        val api = RustPlayerApi(FakeTransport { """{"error":"test failure"}""" })
        try {
            assertEquals("test failure", assertFailsWith<PlayerException> { api.downloads() }.message)
            assertEquals("test failure", assertFailsWith<PlayerException> { api.toggle() }.message)
        } finally {
            api.close()
        }
    }

    private class FakeTransport(private val response: (String) -> String = { "{}" }) : RustTransport {
        val gate = Mutex(locked = true)
        var eventsSent = 0
        var commandCount = 0
        var lastCommand = ""

        override fun command(raw: String): String {
            commandCount += 1
            lastCommand = raw
            return response(raw)
        }

        override fun poll(): String? {
            if (gate.isLocked || eventsSent > 0) return null
            eventsSent += 1
            return """{"status":"playing"}"""
        }

        override fun free() = Unit
    }
}
