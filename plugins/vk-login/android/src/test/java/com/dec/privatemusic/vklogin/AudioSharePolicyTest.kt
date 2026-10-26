package com.dec.privatemusic.vklogin

import java.io.File
import java.nio.file.Files
import org.junit.Assert.*
import org.junit.Test

class AudioSharePolicyTest {
    @Test fun allows_only_prepared_audio_in_the_dedicated_cache_tree() {
        val cache = Files.createTempDirectory("pm-audio-share").toFile()
        try {
            val folder = File(cache, "audio-share/share-abc-1").apply { mkdirs() }
            for (extension in listOf("mp3", "aac", "m4a", "wav", "flac")) {
                val audio = File(folder, "local.$extension").apply { writeText("selected audio") }
                assertEquals(audio.canonicalFile, AudioSharePolicy.checked(cache, audio.absolutePath))
            }
            val file = File(folder, "Artist — Title.aac").apply { writeText("audio") }
            assertEquals(file.canonicalFile, AudioSharePolicy.checked(cache, file.absolutePath))
            for (invalid in listOf(File(cache,"token.mp3"),File(folder,"secret.json"),File(folder,"empty.mp3"))) {
                invalid.parentFile.mkdirs();invalid.writeText(if(invalid.name=="empty.mp3") "" else "private")
                assertTrue(runCatching { AudioSharePolicy.checked(cache, invalid.absolutePath) }.isFailure)
            }
            val outside=File(cache,"outside.mp3").apply {writeText("private")}
            val link=File(folder,"link.mp3")
            Files.createSymbolicLink(link.toPath(),outside.toPath())
            assertTrue(runCatching {AudioSharePolicy.checked(cache,link.absolutePath)}.isFailure)
            assertTrue(runCatching {AudioSharePolicy.checked(cache,File(folder,"../share-abc-1/Artist — Title.aac").absolutePath)}.isFailure)
        } finally {cache.deleteRecursively()}
    }
}
