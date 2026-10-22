package com.dec.privatemusic.vklogin

import java.util.Locale

/** Twin of `session_header` in login.rs, over a `Cookie:` header string. */
object SessionCookies {
    private val SESSION_NAMES = setOf("remixsid", "remixsid6", "remixnsid")

    /**
     * Splits `CookieManager.getCookie` output (`a=b; c=d`) into pairs. The first
     * value of a repeated name wins (the browser lists the most specific first).
     */
    fun parse(header: String?): List<Pair<String, String>> {
        if (header.isNullOrBlank()) return emptyList()
        val seen = HashSet<String>()
        val out = ArrayList<Pair<String, String>>()
        for (part in header.split(';')) {
            val eq = part.indexOf('=')
            if (eq < 0) continue
            val name = part.substring(0, eq).trim()
            if (name.isEmpty() || !seen.add(name)) continue
            out.add(name to part.substring(eq + 1).trim())
        }
        return out
    }

    /**
     * The `Cookie:` header to try, or null while the user is not signed in.
     * Automatic checks need a known session cookie with a value; a manual
     * check ("Done") lets VK judge any non-empty cookie set.
     */
    fun sessionHeader(cookies: List<Pair<String, String>>, manual: Boolean): String? {
        val signedIn = cookies.any { (name, value) -> name.lowercase(Locale.ROOT) in SESSION_NAMES && value.isNotEmpty() }
        if (!(signedIn || (manual && cookies.isNotEmpty()))) return null
        return cookies.joinToString("; ") { (n, v) -> "$n=$v" }
    }
}
