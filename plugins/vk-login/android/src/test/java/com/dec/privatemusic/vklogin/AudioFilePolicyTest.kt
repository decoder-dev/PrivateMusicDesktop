package com.dec.privatemusic.vklogin

import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException

class AudioFilePolicyTest {
    @Test fun copiesAnMp3WithoutTrustingProviderSize() {
        val bytes = "ID3sample audio".toByteArray()
        val output = ByteArrayOutputStream()
        assertEquals(bytes.size.toLong(), AudioFilePolicy.copyMp3(ByteArrayInputStream(bytes), output))
        assertArrayEquals(bytes, output.toByteArray())
    }
    @Test fun rejectsAProviderStreamLargerThanTheLimit() {
        try {
            AudioFilePolicy.copyMp3(ByteArrayInputStream("ID3too large".toByteArray()), ByteArrayOutputStream(), 4)
            fail("Size limit must be enforced")
        } catch (_: IOException) { }
    }
    @Test fun rejectsEmptyOrDisguisedFiles() {
        for (value in listOf("", "ID", "text pretending to be audio")) {
            try {
                AudioFilePolicy.copyMp3(ByteArrayInputStream(value.toByteArray()), ByteArrayOutputStream())
                fail("Invalid MP3 must be rejected")
            } catch (_: IOException) { }
        }
    }
}
