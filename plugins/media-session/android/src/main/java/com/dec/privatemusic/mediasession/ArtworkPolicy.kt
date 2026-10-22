package com.dec.privatemusic.mediasession

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.URI

/** What artwork may be downloaded and how big it may be. URLs are never logged. */
object ArtworkPolicy {
    const val MAX_URL_LENGTH = 2048
    const val MAX_BYTES = 2 * 1024 * 1024
    /** Header-declared size above which the picture is refused before decoding. */
    const val MAX_SOURCE_DIMENSION = 6000
    /** The bitmap kept (and handed to the lock screen) is at most this wide/high. */
    const val TARGET_DIMENSION = 512
    const val MAX_REDIRECTS = 3
    const val CONNECT_TIMEOUT_MS = 5_000
    const val READ_TIMEOUT_MS = 8_000

    /** https only, a real host, no credentials, nothing that points at the device or a private network. */
    fun isAllowedUrl(url: String?): Boolean {
        if (url == null || url.isEmpty() || url.length > MAX_URL_LENGTH) return false
        if (url.any { it.isWhitespace() || it.isISOControl() }) return false
        val uri = try {
            URI(url)
        } catch (_: Exception) {
            return false
        }
        if (!"https".equals(uri.scheme, ignoreCase = true)) return false
        if (uri.userInfo != null) return false
        val host = uri.host?.lowercase() ?: return false
        if (host.isEmpty() || host == "localhost" || host.endsWith(".localhost") || host.endsWith(".local")) return false
        if (isPrivateLiteral(host)) return false
        val port = uri.port
        return port == -1 || port == 443
    }

    private fun isPrivateLiteral(host: String): Boolean {
        if (host.startsWith("[") || host.contains(':')) return true // no IPv6 literals at all
        val parts = host.split('.')
        if (parts.size != 4 || parts.any { it.isEmpty() || !it.all(Char::isDigit) }) return false
        val n = parts.map { it.toIntOrNull() ?: return true }
        val (a, b) = n
        return a == 0 || a == 10 || a == 127 || (a == 169 && b == 254) || (a == 172 && b in 16..31) ||
            (a == 192 && b == 168) || (a == 100 && b in 64..127) || a >= 224
    }

    /** True if a picture of this size may be decoded. */
    fun isAllowedSize(width: Int, height: Int): Boolean =
        width in 1..MAX_SOURCE_DIMENSION && height in 1..MAX_SOURCE_DIMENSION

    /** Power-of-two `inSampleSize` that brings the larger side to at most [target] (at least target/2). */
    fun sampleSize(width: Int, height: Int, target: Int = TARGET_DIMENSION): Int {
        var sample = 1
        var longest = maxOf(width, height)
        while (longest / 2 >= target) {
            longest /= 2
            sample *= 2
        }
        return sample
    }

    /** Reads at most [maxBytes]; null when the stream is longer (a hostile or huge file). */
    fun readLimited(input: InputStream, maxBytes: Int = MAX_BYTES): ByteArray? {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(8 * 1024)
        var total = 0
        while (true) {
            val n = input.read(buffer)
            if (n < 0) break
            total += n
            if (total > maxBytes) return null
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }

    /** Resolves a redirect target against the current URL; null if it is not allowed. */
    fun redirectTarget(current: String, location: String?): String? {
        if (location == null) return null
        val next = try {
            URI(current).resolve(location).toString()
        } catch (_: Exception) {
            return null
        }
        return if (isAllowedUrl(next)) next else null
    }
}
