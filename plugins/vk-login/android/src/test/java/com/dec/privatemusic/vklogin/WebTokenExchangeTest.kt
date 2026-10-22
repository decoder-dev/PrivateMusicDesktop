package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException

class WebTokenExchangeTest {
    private class Fake(private val steps: List<() -> HttpReply>) : HttpTransport {
        var calls = 0
        var lastUrl = ""
        var lastHeaders: Map<String, String> = emptyMap()
        var lastBody = ""
        override fun post(url: String, headers: Map<String, String>, body: String): HttpReply {
            lastUrl = url; lastHeaders = headers; lastBody = body
            return steps[minOf(calls++, steps.size - 1)]()
        }
    }

    private fun reply(status: Int, body: String, cookies: List<String> = emptyList()) = { HttpReply(status, body, cookies) }
    private val okay = """{"type":"okay","data":{"access_token":"0123456789abcdef0123","user_id":1}}"""
    private val noSleep: (Long) -> Unit = {}

    @Test
    fun sendsTheSameRequestAsTheEngine() {
        val fake = Fake(listOf(reply(200, okay, listOf("remixsid=new; Path=/"))))
        val out = WebTokenExchange.exchange("remixsid=old", " UA ", fake, noSleep)
        assertTrue(out is ExchangeOutcome.Okay)
        out as ExchangeOutcome.Okay
        assertEquals(okay, out.json)
        assertEquals(listOf("remixsid=new; Path=/"), out.setCookies)
        assertEquals("https://login.vk.ru/?act=web_token", fake.lastUrl)
        assertEquals("version=1&app_id=6287487", fake.lastBody)
        assertEquals("remixsid=old", fake.lastHeaders["Cookie"])
        assertEquals("UA", fake.lastHeaders["User-Agent"])
        assertEquals("https://vk.ru", fake.lastHeaders["Origin"])
        assertEquals("https://vk.ru/", fake.lastHeaders["Referer"])
        assertEquals("application/x-www-form-urlencoded; charset=utf-8", fake.lastHeaders["Content-Type"])
        assertEquals(1, fake.calls)
        assertFalse(out.toString().contains("access_token"))
    }

    @Test
    fun emptyInputsMeanNotSignedIn() {
        val fake = Fake(listOf(reply(200, okay)))
        assertEquals(ExchangeOutcome.Unauthorized, WebTokenExchange.exchange(" ", "UA", fake, noSleep))
        assertEquals(ExchangeOutcome.Unauthorized, WebTokenExchange.exchange("a=b", "  ", fake, noSleep))
        assertEquals(0, fake.calls)
    }

    @Test
    fun anythingButOkayIsNotSignedIn() {
        assertEquals(ExchangeOutcome.Unauthorized, WebTokenExchange.classify(HttpReply(200, """{"type":"error"}""", emptyList())))
        assertEquals(ExchangeOutcome.Unauthorized, WebTokenExchange.classify(HttpReply(200, "{}", emptyList())))
    }

    @Test
    fun badAnswersAreFormatErrors() {
        assertEquals(ExchangeOutcome.Failed("format"), WebTokenExchange.classify(HttpReply(200, "not json", emptyList())))
        assertEquals(ExchangeOutcome.Failed("format"), WebTokenExchange.classify(HttpReply(403, okay, emptyList())))
        assertEquals(ExchangeOutcome.Failed("format"), WebTokenExchange.classify(HttpReply(302, "", emptyList())))
    }

    @Test
    fun serverTroubleIsRetriedThreeTimes() {
        val fake = Fake(listOf(reply(503, ""), reply(429, ""), reply(200, okay)))
        val slept = ArrayList<Long>()
        assertTrue(WebTokenExchange.exchange("a=b", "UA", fake) { slept.add(it) } is ExchangeOutcome.Okay)
        assertEquals(3, fake.calls)
        assertEquals(listOf(1000L, 2000L), slept)

        val always = Fake(listOf(reply(500, "")))
        assertEquals(ExchangeOutcome.Failed("format"), WebTokenExchange.exchange("a=b", "UA", always, noSleep))
        assertEquals(3, always.calls)
    }

    @Test
    fun transportErrorsAreMappedAndRetried() {
        val flaky = Fake(listOf({ throw SocketTimeoutException() }, reply(200, okay)))
        assertTrue(WebTokenExchange.exchange("a=b", "UA", flaky, noSleep) is ExchangeOutcome.Okay)
        assertEquals(2, flaky.calls)

        fun failing(e: () -> IOException) = Fake(listOf({ throw e() }))
        assertEquals(ExchangeOutcome.Failed("timeout"), WebTokenExchange.exchange("a=b", "UA", failing { SocketTimeoutException() }, noSleep))
        assertEquals(ExchangeOutcome.Failed("offline"), WebTokenExchange.exchange("a=b", "UA", failing { UnknownHostException() }, noSleep))
        assertEquals(ExchangeOutcome.Failed("offline"), WebTokenExchange.exchange("a=b", "UA", failing { ConnectException() }, noSleep))
        val other = failing { IOException("tls") }
        assertEquals(ExchangeOutcome.Failed("network"), WebTokenExchange.exchange("a=b", "UA", other, noSleep))
        assertEquals(3, other.calls)
    }

    @Test
    fun theRealTransportRefusesOtherAddresses() {
        try {
            HttpsTransport.post("https://evil.example/", emptyMap(), "")
            throw AssertionError("must refuse")
        } catch (_: IllegalArgumentException) {
        }
    }
}
