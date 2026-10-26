package com.dec.privatemusic.mediasession

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

/**
 * Downloads one artwork at a time on a background thread and keeps only the latest
 * bitmap. https only, size-limited (see [ArtworkPolicy]); URLs are never logged.
 */
class ArtworkLoader internal constructor(
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "media-artwork").apply { isDaemon = true }
    },
    private val openConnection: (URL) -> HttpURLConnection = { it.openConnection() as HttpURLConnection },
) {
    private val generation = AtomicInteger()

    /** Supersedes any download in flight; its result will be dropped. */
    fun cancel() {
        generation.incrementAndGet()
    }

    /** [onLoaded] runs on the background thread; null bitmap means "no artwork". */
    fun load(url: String, onLoaded: (Bitmap?) -> Unit) {
        val mine = generation.incrementAndGet()
        executor.execute {
            // Rapid track skips can queue several covers behind one slow request.
            // Discard those before opening a connection, not only after decoding.
            if (mine != generation.get()) return@execute
            val bitmap = try {
                download(url, mine)
            } catch (e: Exception) {
                Log.d(TAG, "artwork unavailable (${e.javaClass.simpleName})")
                null
            }
            if (mine == generation.get()) onLoaded(bitmap)
        }
    }

    private fun download(start: String, mine: Int): Bitmap? {
        var current = start
        repeat(ArtworkPolicy.MAX_REDIRECTS + 1) {
            if (mine != generation.get() || !ArtworkPolicy.isAllowedUrl(current)) return null
            val connection = openConnection(URL(current))
            try {
                connection.instanceFollowRedirects = false
                connection.connectTimeout = ArtworkPolicy.CONNECT_TIMEOUT_MS
                connection.readTimeout = ArtworkPolicy.READ_TIMEOUT_MS
                connection.setRequestProperty("Accept", "image/*")
                when (connection.responseCode) {
                    HttpURLConnection.HTTP_OK -> {
                        if (mine != generation.get()) return null
                        return decode(connection)
                    }
                    301, 302, 303, 307, 308 -> {
                        current = ArtworkPolicy.redirectTarget(current, connection.getHeaderField("Location"))
                            ?: return null
                    }
                    else -> return null
                }
            } finally {
                connection.disconnect()
            }
        }
        return null
    }

    private fun decode(connection: HttpURLConnection): Bitmap? {
        val declared = connection.contentLengthLong
        if (declared > ArtworkPolicy.MAX_BYTES) return null
        val bytes = connection.inputStream.use { ArtworkPolicy.readLimited(it) } ?: return null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (!ArtworkPolicy.isAllowedSize(bounds.outWidth, bounds.outHeight)) return null
        val options = BitmapFactory.Options().apply {
            inSampleSize = ArtworkPolicy.sampleSize(bounds.outWidth, bounds.outHeight)
        }
        val decoded = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options) ?: return null
        val longest = maxOf(decoded.width, decoded.height)
        if (longest <= ArtworkPolicy.TARGET_DIMENSION) return decoded
        val scale = ArtworkPolicy.TARGET_DIMENSION.toFloat() / longest
        val scaled = Bitmap.createScaledBitmap(
            decoded,
            (decoded.width * scale).toInt().coerceAtLeast(1),
            (decoded.height * scale).toInt().coerceAtLeast(1),
            true,
        )
        if (scaled !== decoded) decoded.recycle()
        return scaled
    }

    private companion object {
        const val TAG = "MediaSession"
    }
}
