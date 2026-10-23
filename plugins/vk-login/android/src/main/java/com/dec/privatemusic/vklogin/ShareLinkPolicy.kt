package com.dec.privatemusic.vklogin

import java.net.URI

internal object ShareLinkPolicy {
    private fun locator(value: String): Boolean {
        val parts = value.split('_')
        return parts.size == 2 && parts[0].toLongOrNull()?.let { it != 0L } == true
            && parts[1].toLongOrNull()?.let { it > 0L } == true
            && parts[0].all { it in '0'..'9' || it == '-' } && parts[1].all { it in '0'..'9' }
    }
    fun accepts(raw: String): Boolean = try {
        val url = URI(raw)
        if (raw.length > 2048 || url.scheme != "https" || url.host != "vk.com" || url.rawUserInfo != null
            || url.port != -1 || url.fragment != null) false
        else if (url.path.startsWith("/audio")) locator(url.path.removePrefix("/audio")) && url.rawQuery == null
        else {
            val prefix = listOf("/music/playlist/", "/music/album/").firstOrNull { url.path.startsWith(it) }
            prefix != null && locator(url.path.removePrefix(prefix)) &&
                (url.rawQuery == null || Regex("access_key=[A-Za-z0-9]{1,256}").matches(url.rawQuery))
        }
    } catch (_: Exception) { false }
}
