package com.dec.privatemusic.compose.app

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.webkit.CookieManager
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import com.dec.privatemusic.player.Session
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import javax.net.ssl.HttpsURLConnection

/** Isolated VK sign-in: password and 2FA stay on vk.ru, only the session returns to the app. */
class VkLoginActivity : Activity() {
    companion object {
        const val USER_AGENT = "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36"
        const val API_USER_AGENT = "KateMobileAndroid/56 lite-460 (Android 4.4.2; SDK 19; x86; unknown Android SDK built for x86; ru)"
        const val TOKEN_URL = "https://login.vk.ru/?act=web_token"
    }

    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private var webView: WebView? = null
    private var finished = false
    private var exchanging = false
    private var lastHeader: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val close = Button(this).apply { text = "✕"; setOnClickListener { finish() } }
        val done = Button(this).apply { text = "Продолжить"; setOnClickListener { checkSession() } }
        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(close, LinearLayout.LayoutParams(dp(56), dp(56)))
            addView(done, LinearLayout.LayoutParams(0, dp(56), 1f))
        }
        val wv = WebView(this)
        webView = wv
        wv.settings.apply {
            @SuppressLint("SetJavaScriptEnabled") javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            savePassword = false
            saveFormData = false
            mixedContentMode = android.webkit.WebSettings.MIXED_CONTENT_NEVER_ALLOW
            safeBrowsingEnabled = true
            userAgentString = USER_AGENT
        }
        val cookies = CookieManager.getInstance()
        cookies.setAcceptCookie(true)
        cookies.setAcceptThirdPartyCookies(wv, true)
        wv.webViewClient = WebViewClient()
        wv.loadUrl("https://vk.com/audios")
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(bar)
            addView(wv, LinearLayout.LayoutParams(0, 0, 1f))
        }
        setContentView(root)
        main.postDelayed(object : Runnable {
            override fun run() {
                if (!finished) {
                    checkSession()
                    main.postDelayed(this, 1500)
                }
            }
        }, 1500)
    }

    private fun checkSession() {
        if (finished || exchanging) return
        val manager = CookieManager.getInstance()
        val pairs = SessionCookies.merge(manager.getCookie(TOKEN_URL), manager.getCookie("https://vk.ru/")
        val header = SessionCookies.sessionHeader(pairs, true) ?: return
        if (header == lastHeader) return
        lastHeader = header
        exchanging = true
        worker.execute {
            val outcome = runCatching { exchange(header, USER_AGENT) }
            main.post {
                exchanging = false
                outcome.onSuccess { result ->
                    if (result != null) finishWith(result)
                    else lastHeader = null
                }.onFailure { lastHeader = null }
            }
        }
    }

    private fun finishWith(session: Session) {
        if (finished) return
        finished = true
        val json = JSONObject().apply {
            put("access_token", session.access_token)
            put("user_id", session.user_id)
            put("expires_at", session.expires_at)
            put("api_user_agent", session.api_user_agent)
            put("refresh_cookie", session.refresh_cookie)
            put("web_user_agent", session.web_user_agent)
        }
        setResult(RESULT_OK, Intent().putExtra("session", json.toString()))
        CookieManager.getInstance().removeAllCookies(null)
        finish()
    }

    override fun onDestroy() {
        finished = true
        worker.shutdownNow()
        webView?.destroy()
        webView = null
        super.onDestroy()
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
}

private object SessionCookies {
    private val names = setOf("remixsid", "remixsid6", "remixnsid")

    fun merge(vararg headers: String?): List<Pair<String, String> {
        val result = LinkedHashMap<String, String>()
        headers.filterNotNull().forEach { header ->
            header.split(';').forEach { part ->
                val eq = part.indexOf('=')
                if (eq > 0) {
                    val name = part.substring(0, eq).trim()
                    if (name.isNotEmpty() && !result.containsKey(name)) result[name] = part.substring(eq + 1).trim()
                }
            }
        }
        return result.entries.map { it.key to it.value }
    }

    fun sessionHeader(cookies: List<Pair<String, String>>, manual: Boolean): String? {
        val signedIn = cookies.any { (name, value) -> name.lowercase() in names && value.isNotEmpty() }
        if (!(signedIn || (manual && cookies.isNotEmpty()))) return null
        return cookies.joinToString("; ") { (name, value) -> "$name=$value" }
    }
}

private data class WebReply(val status: Int, val body: String, val setCookies: List<String>)

private fun exchange(cookieHeader: String, userAgent: String): Session? {
    val headers = linkedMapOf(
        "Content-Type" to "application/x-www-form-urlencoded; charset=utf-8",
        "Accept" to "application/json",
        "Origin" to "https://vk.ru",
        "Referer" to "https://vk.ru/",
        "Cookie" to cookieHeader,
        "User-Agent" to userAgent,
    )
    repeat(3) { attempt ->
        val reply = post(headers, "version=1&app_id=6287487")
        if ((reply.status == 429 || reply.status in 500..599) && attempt < 2) {
            Thread.sleep(1000L * (attempt + 1))
            return@repeat
        }
        if (reply.status !in 200..299) return null
        val body = JSONObject(reply.body)
        if (body.optString("type") != "okay") return null
        val data = body.getJSONObject("data")
        val token = data.getString("access_token")
        if (token.length < 16) return null
        return Session(
            access_token = token,
            user_id = data.optLong("user_id").takeIf { it != 0L },
            expires_at = data.optLong("expires").takeIf { it > 0 }?.let { raw ->
                if (raw > 1_000_000_000) raw else System.currentTimeMillis() / 1000 + raw
            },
            api_user_agent = VkLoginActivity.API_USER_AGENT,
            refresh_cookie = mergeCookies(cookieHeader, reply.setCookies),
            web_user_agent = userAgent,
        )
    }
    return null
}

private fun post(headers: Map<String, String>, body: String): WebReply {
    val connection = URL(VkLoginActivity.TOKEN_URL).openConnection() as HttpsURLConnection
    return try {
        connection.requestMethod = "POST"
        connection.instanceFollowRedirects = false
        connection.connectTimeout = 15_000
        connection.readTimeout = 30_000
        connection.doOutput = true
        connection.useCaches = false
        headers.forEach { (key, value) -> connection.setRequestProperty(key, value) }
        connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
        val status = connection.responseCode
        val stream = if (status >= 400) connection.errorStream else connection.inputStream
        val text = stream?.use { it.readBytes().toString(Charsets.UTF_8) } ?: ""
        val cookies = connection.headerFields.entries
            .filter { it.key != null && it.key.equals("Set-Cookie", ignoreCase = true) }
            .flatMap { it.value ?: emptyList() }
        WebReply(status, text, cookies)
    } finally {
        connection.disconnect()
    }
}

private fun mergeCookies(original: String, setCookies: List<String>): String {
    val values = LinkedHashMap<String, String>()
    (original.split(';') + setCookies.map { it.substringBefore(';') }).forEach { part ->
        val eq = part.indexOf('=')
        if (eq > 0) values[part.substring(0, eq).trim()] = part.substring(eq + 1).trim()
    }
    return values.entries.joinToString("; ") { "${it.key}=${it.value}" }
}
