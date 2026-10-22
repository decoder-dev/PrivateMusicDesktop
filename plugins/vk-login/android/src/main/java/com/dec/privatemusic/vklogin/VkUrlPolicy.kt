package com.dec.privatemusic.vklogin

import java.util.Locale

/**
 * Which addresses the sign-in WebView may go to. This is the Kotlin twin of
 * `is_vk_host` / `is_allowed_navigation` in `src-tauri/src/login.rs`, and
 * deliberately a little stricter (see the notes on each rule). Pure logic: no
 * Android classes, so it is unit-tested on the JVM.
 */
object VkUrlPolicy {
    const val LOGIN_URL = "https://vk.ru/login"

    /** The host whose cookies carry the session (login.vk.ru is vk.ru's). */
    const val TOKEN_COOKIE_URL = "https://login.vk.ru/"

    private const val MAX_URL = 8192

    /** vk.ru, vk.com and their subdomains, nothing else. `host` must be ASCII. */
    fun isVkHost(host: String?): Boolean {
        if (host == null || host.isEmpty() || host.length > 253) return false
        val h = host.lowercase(Locale.ROOT)
        // Letters, digits, '-' and '.' only: no '%', '@', ':', '\\', IDN or control characters.
        if (!h.all { it in 'a'..'z' || it in '0'..'9' || it == '-' || it == '.' }) return false
        // No empty labels (".vk.ru", "vk..ru") and no trailing dot ("vk.ru.": the Rust rule rejects it too).
        if (h.split('.').any { it.isEmpty() }) return false
        return h == "vk.ru" || h.endsWith(".vk.ru") || h == "vk.com" || h.endsWith(".vk.com")
    }

    /**
     * https on a VK host. Also accepted, as on the desktop: `about:blank` /
     * `about:srcdoc` frames, and `blob:https://<vk host>/<id>` object URLs made
     * by VK's own page. Stricter than the desktop: any user-info (`user@`), and
     * any port other than 443, are refused.
     */
    fun isAllowedNavigation(url: String?): Boolean {
        if (url == null || url.isEmpty() || url.length > MAX_URL) return false
        // Whitespace, control characters, backslashes and non-ASCII are never part of
        // an address the browser would have normalised: refuse them before parsing.
        if (url.any { it <= ' ' || it == '\\' || it.code >= 0x7f }) return false
        val colon = url.indexOf(':')
        if (colon <= 0) return false
        val scheme = url.substring(0, colon).lowercase(Locale.ROOT)
        val rest = url.substring(colon + 1)
        return when (scheme) {
            "https" -> httpsHost(rest)?.let(::isVkHost) ?: false
            "about" -> {
                val path = rest.substringBefore('#').substringBefore('?').lowercase(Locale.ROOT)
                path == "blank" || path == "srcdoc"
            }
            "blob" -> rest.startsWith("https:", ignoreCase = true) && isAllowedNavigation(rest)
            else -> false
        }
    }

    /** `//host[:443]/...` -> host, or null when the authority is not acceptable. */
    private fun httpsHost(afterScheme: String): String? {
        if (!afterScheme.startsWith("//")) return null
        val authority = afterScheme.substring(2).takeWhile { it != '/' && it != '?' && it != '#' }
        if (authority.isEmpty() || '@' in authority || authority.startsWith("[")) return null
        val colon = authority.indexOf(':')
        if (colon < 0) return authority
        val port = authority.substring(colon + 1)
        // "https://vk.ru:/" is the default port written out; anything but 443 is refused.
        if (port.isNotEmpty() && port != "443") return null
        return authority.substring(0, colon)
    }

    /** True for an https page on a VK host (what "VK's own page loaded" means). */
    fun isVkHttpsPage(url: String?): Boolean =
        url != null && url.startsWith("https:", ignoreCase = true) && isAllowedNavigation(url)

    /** Lower-cased host of an https URL that passed [isAllowedNavigation], else null. */
    fun vkHostOf(url: String?): String? {
        if (!isVkHttpsPage(url)) return null
        return httpsHost(url!!.substring(url.indexOf(':') + 1))?.lowercase(Locale.ROOT)
    }

    /**
     * What may be written to a log about an address: its scheme and host only.
     * Sign-in addresses can carry one-time codes, so never the path, query,
     * fragment or user-info.
     */
    fun describeForLog(url: String?): String {
        if (url == null) return "unknown page"
        val colon = url.indexOf(':')
        if (colon <= 0 || colon > 16) return "unparsable page"
        val scheme = url.substring(0, colon).lowercase(Locale.ROOT)
        if (!scheme.all { it in 'a'..'z' || it in '0'..'9' || it == '+' || it == '-' || it == '.' }) return "unparsable page"
        var rest = url.substring(colon + 1)
        if (scheme == "blob") {
            return "blob page"
        }
        if (!rest.startsWith("//")) return "$scheme page"
        rest = rest.substring(2).takeWhile { it != '/' && it != '?' && it != '#' }
        val hostPort = rest.substringAfterLast('@')
        val host = if (hostPort.startsWith("[")) "ipv6" else hostPort.substringBefore(':')
        val safe = host.lowercase(Locale.ROOT).filter { it in 'a'..'z' || it in '0'..'9' || it == '-' || it == '.' }
            .take(80)
        return "$scheme page on ${safe.ifEmpty { "?" }}"
    }

    /**
     * Web storage of this origin is wiped before and after a sign-in unless it
     * belongs to the host app itself: the Tauri UI keeps its settings in the
     * same WebView profile (`http(s)://tauri.localhost`), and `deleteAllData`
     * would erase them. Anything on the public web goes.
     */
    fun shouldWipeOrigin(origin: String?): Boolean {
        if (origin == null) return false
        val o = origin.lowercase(Locale.ROOT)
        val colon = o.indexOf(':')
        if (colon <= 0) return false
        val scheme = o.substring(0, colon)
        if (scheme != "https" && scheme != "http") return false
        val host = o.substring(colon + 1).removePrefix("//").takeWhile { it != '/' && it != ':' }
        if (host.isEmpty()) return false
        return !(host == "localhost" || host.endsWith(".localhost") || host == "127.0.0.1")
    }
}
