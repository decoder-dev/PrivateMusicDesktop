package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LoginEventTest {
    @Test
    fun theWireFormatIsWhatRustParses() {
        assertEquals("""{"event":"cancelled"}""", LoginEvent.Cancelled.toJson().toString())
        assertEquals("""{"event":"loadFailed"}""", LoginEvent.LoadFailed.toJson().toString())
        assertEquals("""{"event":"loadOk"}""", LoginEvent.LoadOk.toJson().toString())
        assertEquals("pending", LoginEvent.Pending().toJson().getString("event"))
        assertEquals("unauthorized", LoginEvent.Pending().toJson().getString("reason"))
        assertEquals("timeout", LoginEvent.Error("timeout").toJson().getString("kind"))
        assertEquals("vk.ru", LoginEvent.SmokeLoaded("vk.ru").toJson().getString("host"))
    }

    @Test
    fun aSuccessCarriesTheSessionAndPrintsNothing() {
        val event = LoginEvent.Success("remixsid=SECRET", """{"type":"okay"}""", listOf("a=b; Path=/"), "UA")
        val json = event.toJson()
        assertEquals("success", json.getString("event"))
        assertEquals("remixsid=SECRET", json.getString("cookieHeader"))
        assertEquals("""{"type":"okay"}""", json.getString("webTokenJson"))
        assertEquals("a=b; Path=/", json.getJSONArray("setCookies").getString(0))
        assertEquals("UA", json.getString("userAgent"))
        assertFalse(event.toString().contains("SECRET"))
        assertTrue(event.toString().contains("redacted"))
    }
}
