package com.dec.privatemusic.vklogin

import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer

/** Container metadata only: audio samples are never decoded or re-encoded. */
internal object AudioExportTags {
    data class Metadata(val title: String, val artist: String, val album: String, val lyrics: String, val cover: ByteArray?)
    private fun bytes(text: String) = text.toByteArray(Charsets.UTF_8)
    private fun be(value: Int) = ByteBuffer.allocate(4).putInt(value).array()
    private fun sync(value: Int) = byteArrayOf(((value shr 21) and 127).toByte(), ((value shr 14) and 127).toByte(), ((value shr 7) and 127).toByte(), (value and 127).toByte())
    private fun atom(name: String, data: ByteArray) = be(data.size + 8) + name.toByteArray(Charsets.ISO_8859_1) + data
    private fun mp4Tag(name: String, value: ByteArray, kind: Int = 1) = atom(name, atom("data", be(kind) + be(0) + value))
    fun mp4(file: File, metadata: Metadata) {
        val fields = listOf("©nam" to metadata.title, "©ART" to metadata.artist, "©alb" to metadata.album, "©lyr" to metadata.lyrics)
        var tags = fields.filter { it.second.isNotBlank() }.fold(byteArrayOf()) { data, pair -> data + mp4Tag(pair.first, bytes(pair.second)) }
        metadata.cover?.let { tags += mp4Tag("covr", it, 13) }
        val handler = atom("hdlr", be(0) + be(0) + bytes("mdir") + ByteArray(13))
        val extra = atom("udta", atom("meta", be(0) + handler + atom("ilst", tags)))
        RandomAccessFile(file, "rw").use { output ->
            var offset = 0L
            while (offset + 8 <= output.length()) {
                output.seek(offset)
                val size = output.readInt().toLong() and 0xffffffffL
                val kind = ByteArray(4).also { output.readFully(it) }.toString(Charsets.ISO_8859_1)
                require(size >= 8 && offset + size <= output.length())
                if (kind == "moov") {
                    // MediaMuxer places moov last. Appending changes no media/chunk offsets.
                    require(offset + size == output.length() && size + extra.size <= Int.MAX_VALUE)
                    output.seek(output.length()); output.write(extra)
                    output.seek(offset); output.writeInt((size + extra.size).toInt())
                    return
                }
                offset += size
            }
            error("MP4 metadata container missing")
        }
    }
    fun mp3(input: File, output: File, metadata: Metadata) {
        fun frame(name: String, value: ByteArray) = bytes(name) + sync(value.size) + byteArrayOf(0, 0) + value
        var frames = listOf("TIT2" to metadata.title, "TPE1" to metadata.artist, "TALB" to metadata.album)
            .filter { it.second.isNotBlank() }.fold(byteArrayOf()) { data, pair -> data + frame(pair.first, byteArrayOf(3) + bytes(pair.second)) }
        if (metadata.lyrics.isNotBlank()) frames += frame("USLT", byteArrayOf(3) + bytes("und") + byteArrayOf(0) + bytes(metadata.lyrics))
        metadata.cover?.let { frames += frame("APIC", byteArrayOf(3) + bytes("image/jpeg") + byteArrayOf(0, 3, 0) + it) }
        input.inputStream().buffered().use { source ->
            source.mark(10)
            val header = ByteArray(10)
            if (source.read(header) == 10 && header.copyOfRange(0, 3).contentEquals(bytes("ID3"))) {
                require(header[3].toInt() in 2..4 && header.sliceArray(6..9).all { it.toInt() and 128 == 0 })
                var size = header.sliceArray(6..9).fold(0) { value, part -> (value shl 7) or part.toInt() }
                if (header[3].toInt() == 4 && header[5].toInt() and 16 != 0) size += 10
                require(size <= 6 * 1024 * 1024 && size + 10L < input.length())
                var remaining = size.toLong()
                while (remaining > 0) { val skipped = source.skip(remaining); require(skipped > 0); remaining -= skipped }
            } else source.reset()
            output.outputStream().buffered().use { target ->
                target.write(bytes("ID3") + byteArrayOf(4, 0, 0) + sync(frames.size) + frames)
                source.copyTo(target)
            }
        }
    }
}
