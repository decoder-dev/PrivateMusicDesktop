package com.dec.privatemusic.vklogin

import java.io.InputStream
import java.io.OutputStream
import java.io.IOException

/** Copy only the user-selected MP3, with a bound even when a provider reports no size. */
internal object AudioFilePolicy {
    const val MAX_BYTES = 200_000_000L
    fun copyMp3(input: InputStream, output: OutputStream, maximum: Long = MAX_BYTES): Long {
        val header = ByteArray(3)
        var offset = 0
        while (offset < header.size) {
            val count = input.read(header, offset, header.size - offset)
            if (count <= 0) throw IOException("Invalid MP3")
            offset += count
        }
        val id3 = header.contentEquals(byteArrayOf(73, 68, 51))
        val frame = (header[0].toInt() and 255) == 255 && (header[1].toInt() and 224) == 224
        if ((!id3 && !frame) || maximum < header.size) throw IOException("Invalid MP3")
        output.write(header)
        var total = header.size.toLong()
        val buffer = ByteArray(64 * 1024)
        while (true) {
            if (Thread.currentThread().isInterrupted) throw IOException("Cancelled")
            val count = input.read(buffer)
            if (count < 0) break
            total += count
            if (total > maximum) throw IOException("Audio exceeds size limit")
            output.write(buffer, 0, count)
        }
        return total
    }
}
