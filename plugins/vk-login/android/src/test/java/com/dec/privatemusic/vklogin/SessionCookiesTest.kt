package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class SessionCookiesTest {
    @Test
    fun parsesACookieHeader() {
        assertEquals(
            listOf("remixlang" to "0", "remixsid" to "a=b=c", "empty" to ""),
            SessionCookies.parse("remixlang=0; remixsid=a=b=c;  empty= ; novalue; =x"),
        )
        assertEquals(emptyList<Pair<String, String>>(), SessionCookies.parse(null))
        assertEquals(emptyList<Pair<String, String>>(), SessionCookies.parse("  "))
    }

    @Test
    fun theFirstValueOfARepeatedNameWins() {
        assertEquals(listOf("remixsid" to "login", "x" to "1"), SessionCookies.parse("remixsid=login; x=1; remixsid=root"))
    }

    @Test
    fun aSessionCookieIsRequiredUnlessChecked() {
        val anonymous = listOf("remixlang" to "0", "remixsid" to "")
        assertNull(SessionCookies.sessionHeader(anonymous, false))
        assertNotNull("a manual check lets VK judge an unrecognised session", SessionCookies.sessionHeader(anonymous, true))
        assertNull(SessionCookies.sessionHeader(emptyList(), true))
        val signedIn = listOf("remixlang" to "0", "remixsid" to "abc")
        assertEquals("remixlang=0; remixsid=abc", SessionCookies.sessionHeader(signedIn, false))
        assertEquals("REMIXSID6=z", SessionCookies.sessionHeader(listOf("REMIXSID6" to "z"), false))
        assertEquals("remixnsid=1", SessionCookies.sessionHeader(listOf("remixnsid" to "1"), false))
    }
}
