package com.dec.privatemusic.vklogin

import org.json.JSONObject
import java.io.IOException
import java.net.ConnectException
import java.net.HttpURLConnection
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.URL
import java.net.UnknownHostException
import javax.net.ssl.HttpsURLConnection

/** What VK answered to `login.vk.ru/?act=web_token`. */
class HttpReply(val status: Int, val body: String, val setCookies: List<String>)

/** One HTTPS POST. Faked in the unit tests. */
interface HttpTransport {
    @Throws(IOException::class)
    fun post(url: String, headers: Map<String, String>, body: String): HttpReply
}

sealed class ExchangeOutcome {
    /** VK calls the web session signed in. `json` is its answer, for Rust to decode. */
    class Okay(val json: String, val setCookies: List<String>) : ExchangeOutcome() {
        override fun toString() = "Okay(<redacted>)"
    }

    /** A cookie exists but VK does not call it signed in (yet): a 2FA or captcha step. */
    object Unauthorized : ExchangeOutcome()

    /** Everything else; `kind` is an engine error kind (`offline`, `timeout`, `network`, `format`). */
    data class Failed(val kind: String) : ExchangeOutcome()
}

/**
 * The token exchange of `engine/src/vk/auth.rs::exchange_at`, done from the
 * sign-in screen so a 2FA step does not cost the user the page: the same URL,
 * form, headers, three attempts, no redirects. The answer is decoded by the
 * engine (`session_from_web_token`); this class only decides "signed in or not".
 */
object WebTokenExchange {
    const val TOKEN_URL = "https://login.vk.ru/?act=web_token"
    private const val WEB_CLIENT_ID = "6287487"
    private const val ATTEMPTS = 3
    private const val MAX_BODY = 256 * 1024

    fun exchange(
        cookieHeader: String,
        userAgent: String,
        transport: HttpTransport = HttpsTransport,
        sleep: (Long) -> Unit = { Thread.sleep(it) },
    ): ExchangeOutcome {
        val cookies = cookieHeader.trim()
        val agent = userAgent.trim()
        if (cookies.isEmpty() || agent.isEmpty()) return ExchangeOutcome.Unauthorized
        val headers = linkedMapOf(
            "Content-Type" to "application/x-www-form-urlencoded; charset=utf-8",
            "Accept" to "application/json",
            "Origin" to "https://vk.ru",
            "Referer" to "https://vk.ru/",
            "Cookie" to cookies,
            "User-Agent" to agent,
        )
        var attempt = 0
        while (true) {
            attempt++
            val last = attempt >= ATTEMPTS
            val reply = try {
                transport.post(TOKEN_URL, headers, "version=1&app_id=$WEB_CLIENT_ID")
            } catch (e: IOException) {
                val kind = errorKind(e)
                if (!last && kind != "format") {
                    sleep(retryDelayMs(attempt))
                    continue
                }
                return ExchangeOutcome.Failed(kind)
            }
            if ((reply.status == 429 || reply.status in 500..599) && !last) {
                sleep(retryDelayMs(attempt))
                continue
            }
            return classify(reply)
        }
    }

    internal fun retryDelayMs(attempt: Int): Long = 1000L * (1L shl (attempt - 1).coerceIn(0, 4))

    internal fun classify(reply: HttpReply): ExchangeOutcome {
        LoginTrace.note("web_token answered HTTP ${reply.status}")
        if (reply.status !in 200..299) return ExchangeOutcome.Failed("format")
        val type = try {
            JSONObject(reply.body).optString("type", "")
        } catch (_: Exception) {
            return ExchangeOutcome.Failed("format")
        }
        LoginTrace.note("web_token type=" + type.take(40))
        // Anything but "okay" means the page is not signed in (or VK refused).
        if (type != "okay") return ExchangeOutcome.Unauthorized
        return ExchangeOutcome.Okay(reply.body, reply.setCookies)
    }

    internal fun errorKind(e: Throwable): String = when (e) {
        is SocketTimeoutException -> "timeout"
        is UnknownHostException, is ConnectException, is NoRouteToHostException -> "offline"
        else -> "network"
    }
}

/** Real transport. Only ever used for [WebTokenExchange.TOKEN_URL]. */
object HttpsTransport : HttpTransport {
    override fun post(url: String, headers: Map<String, String>, body: String): HttpReply {
        require(url == WebTokenExchange.TOKEN_URL) { "unexpected address" }
        val conn = URL(url).openConnection() as HttpsURLConnection
        try {
            conn.requestMethod = "POST"
            conn.instanceFollowRedirects = false
            conn.connectTimeout = 15_000
            conn.readTimeout = 30_000
            conn.doOutput = true
            conn.useCaches = false
            headers.forEach { (k, v) -> conn.setRequestProperty(k, v) }
            conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            val status = conn.responseCode
            val stream = if (status >= 400) conn.errorStream else conn.inputStream
            val text = stream?.use { s ->
                val buffer = java.io.ByteArrayOutputStream()
                val chunk = ByteArray(8192)
                while (true) {
                    val n = s.read(chunk)
                    if (n < 0) break
                    buffer.write(chunk, 0, n)
                    if (buffer.size() > 256 * 1024) throw IOException("answer too large")
                }
                buffer.toString("UTF-8")
            } ?: ""
            val setCookies = conn.headerFields.entries
                .filter { it.key != null && it.key.equals("Set-Cookie", ignoreCase = true) }
                .flatMap { it.value ?: emptyList() }
            return HttpReply(status, text, setCookies)
        } finally {
            conn.disconnect()
        }
    }
}
