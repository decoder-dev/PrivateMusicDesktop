package com.dec.privatemusic.vklogin

import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaCodec
import android.media.MediaMuxer
import java.io.File
import java.nio.ByteBuffer

internal object AudioShareEnricher {
    fun prepare(input: File, title: String, artist: String, album: String, lyrics: String, lrc: String, cover: File?): List<File> {
        require(lyrics.toByteArray().size <= 150_000 && lrc.toByteArray().size <= 200_000)
        if (input.extension in setOf("m4a", "wav", "flac")) {
            val files = mutableListOf(input)
            if (lrc.isNotBlank() || lyrics.isNotBlank()) {
                val sidecar = File(input.parentFile, input.nameWithoutExtension + if (lrc.isNotBlank()) ".lrc" else ".txt")
                sidecar.writeText(if (lrc.isNotBlank()) lrc else lyrics, Charsets.UTF_8)
                files.add(sidecar)
            }
            return files
        }
        val image = cover?.also { require(it.parentFile == input.parentFile && it.extension == "jpg" && it.length() in 1..3_145_728) }?.readBytes()
        val metadata = AudioExportTags.Metadata(title.take(1000), artist.take(1000), album.take(1000), lyrics, image)
        val output = File(input.parentFile, input.nameWithoutExtension + if (input.extension == "aac") ".m4a" else ".tagged.mp3")
        try {
            if (input.extension == "aac") {
                remux(input, output)
                AudioExportTags.mp4(output, metadata)
            } else AudioExportTags.mp3(input, output, metadata)
            require(output.isFile && output.length() in 1..AudioSharePolicy.MAX_BYTES)
            val audio = if (input.extension == "mp3") {
                require(input.delete() && output.renameTo(input))
                input
            } else output
            val files = mutableListOf(audio)
            if (lrc.isNotBlank() || lyrics.isNotBlank()) {
                val sidecar = File(input.parentFile, input.nameWithoutExtension + if (lrc.isNotBlank()) ".lrc" else ".txt")
                sidecar.writeText(if (lrc.isNotBlank()) lrc else lyrics, Charsets.UTF_8)
                files.add(sidecar)
            }
            return files
        } catch (error: Exception) { output.delete(); throw error }
    }
    private fun remux(input: File, output: File) {
        val extractor = MediaExtractor()
        var muxer: MediaMuxer? = null
        try {
            extractor.setDataSource(input.absolutePath)
            val index = (0 until extractor.trackCount).firstOrNull { extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME) == "audio/mp4a-latm" }
                ?: error("AAC audio track missing")
            extractor.selectTrack(index)
            val format = extractor.getTrackFormat(index)
            val capacity = if (format.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) format.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE) else 1024 * 1024
            require(capacity in 1..8 * 1024 * 1024)
            val data = ByteBuffer.allocateDirect(maxOf(capacity, 64 * 1024))
            val info = MediaCodec.BufferInfo()
            val writer = MediaMuxer(output.absolutePath, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            muxer = writer
            val track = writer.addTrack(format)
            writer.start()
            var frames = 0
            while (true) {
                data.clear()
                val size = extractor.readSampleData(data, 0)
                if (size < 0) break
                require(size <= data.capacity() && ++frames <= 5_000_000)
                info.set(0, size, extractor.sampleTime.coerceAtLeast(0), extractor.sampleFlags)
                writer.writeSampleData(track, data, info)
                if (!extractor.advance()) break
            }
            require(frames > 0)
            writer.stop()
        } finally { extractor.release(); muxer?.release() }
    }
}
