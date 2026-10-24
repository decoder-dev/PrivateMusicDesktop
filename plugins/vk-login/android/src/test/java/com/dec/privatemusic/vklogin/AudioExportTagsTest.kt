package com.dec.privatemusic.vklogin

import java.io.File
import java.nio.ByteBuffer
import java.nio.file.Files
import org.junit.Assert.*
import org.junit.Test

class AudioExportTagsTest {
    private fun metadata() = AudioExportTags.Metadata("Песня", "Artist", "Album", "Words", byteArrayOf(-1, -40, -1, -39))
    @Test fun mp3_embeds_unicode_metadata_and_preserves_every_audio_byte() {
        val dir = Files.createTempDirectory("pm-export").toFile()
        try {
            val audio = ByteArray(4096) { (it % 256).toByte() }
            val source = File(dir, "source.mp3").apply { writeBytes(audio) }
            val result = File(dir, "export.mp3")
            AudioExportTags.mp3(source, result, metadata())
            val bytes = result.readBytes()
            assertEquals("ID3", bytes.copyOfRange(0, 3).toString(Charsets.UTF_8))
            val size = bytes.sliceArray(6..9).fold(0) { n, v -> (n shl 7) or v.toInt() }
            assertArrayEquals(audio, bytes.copyOfRange(size + 10, bytes.size))
            assertTrue(bytes.toString(Charsets.UTF_8).contains("Песня"))
            assertTrue(bytes.toString(Charsets.UTF_8).contains("APIC"))
            assertTrue(bytes.toString(Charsets.UTF_8).contains("USLT"))
            // Replacing a previous tag must not add its old frames to the audio payload.
            val again = File(dir, "again.mp3")
            AudioExportTags.mp3(result, again, metadata())
            assertArrayEquals(bytes, again.readBytes())
        } finally { dir.deleteRecursively() }
    }
    @Test fun mp4_metadata_extends_only_the_last_moov_and_keeps_media_offsets() {
        val dir = Files.createTempDirectory("pm-m4a").toFile()
        try {
            fun atom(name: String, value: ByteArray) = ByteBuffer.allocate(4).putInt(value.size + 8).array() + name.toByteArray() + value
            val media = atom("mdat", ByteArray(100) { it.toByte() })
            val file = File(dir, "song.m4a").apply { writeBytes(media + atom("moov", byteArrayOf())) }
            AudioExportTags.mp4(file, metadata())
            val bytes = file.readBytes()
            assertArrayEquals(media, bytes.copyOfRange(0, media.size))
            assertEquals(bytes.size - media.size, ByteBuffer.wrap(bytes, media.size, 4).int)
            assertTrue(bytes.toString(Charsets.ISO_8859_1).contains("ilst"))
            assertTrue(bytes.toString(Charsets.ISO_8859_1).contains("covr"))
            assertTrue(bytes.toString(Charsets.UTF_8).contains("Песня"))
            val beforeMedia = File(dir, "invalid.m4a").apply { writeBytes(atom("moov", byteArrayOf()) + media) }
            assertTrue(runCatching { AudioExportTags.mp4(beforeMedia, metadata()) }.isFailure)
        } finally { dir.deleteRecursively() }
    }
    @Test fun standard_media_reader_recognizes_real_mp4_and_mp3_tags_without_changing_audio() {
        org.junit.Assume.assumeTrue(File("/usr/bin/ffmpeg").canExecute() && File("/usr/bin/ffprobe").canExecute())
        val dir = Files.createTempDirectory("pm-export-probe").toFile()
        fun run(vararg args: String): String {
            val process = ProcessBuilder(*args).redirectErrorStream(true).start()
            val text = process.inputStream.bufferedReader().readText()
            assertEquals(text, 0, process.waitFor())
            return text
        }
        try {
            for (extension in listOf("m4a", "mp3")) {
                val source = File(dir, "source.$extension")
                run("/usr/bin/ffmpeg", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-ar", "48000", "-ac", "2", source.absolutePath)
                val result = File(dir, "export.$extension")
                if (extension == "m4a") { source.copyTo(result); AudioExportTags.mp4(result, metadata().copy(cover = null)) }
                else AudioExportTags.mp3(source, result, metadata().copy(cover = null))
                val tags = run("/usr/bin/ffprobe", "-v", "error", "-show_entries", "format_tags", "-of", "json", result.absolutePath)
                assertTrue("$extension: $tags", tags.contains("Песня") && tags.contains("Artist") && tags.contains("Album") && tags.contains("Words"))
                val decodedSource = run("/usr/bin/ffmpeg", "-v", "error", "-i", source.absolutePath, "-map", "0:a:0", "-f", "md5", "-")
                val decodedResult = run("/usr/bin/ffmpeg", "-v", "error", "-i", result.absolutePath, "-map", "0:a:0", "-f", "md5", "-")
                assertEquals(decodedSource, decodedResult)
            }
        } finally { dir.deleteRecursively() }
    }

}
