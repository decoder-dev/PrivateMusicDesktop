package com.dec.privatemusic.vklogin

/** A lyric can be copied explicitly, without changing the stricter VK-link clipboard contract. */
internal object ClipboardTextPolicy {
    const val MAX_BYTES = 150_000
    fun accepts(text: String): Boolean = text.length <= MAX_BYTES && text.isNotBlank() && text.toByteArray(Charsets.UTF_8).size <= MAX_BYTES
}
