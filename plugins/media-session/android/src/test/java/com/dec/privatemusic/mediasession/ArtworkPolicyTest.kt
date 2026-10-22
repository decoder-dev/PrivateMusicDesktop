package com.dec.privatemusic.mediasession

import java.io.ByteArrayInputStream
import java.io.InputStream
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ArtworkPolicyTest {
    @Test
    fun onlyHttpsUrlsAreAllowed() {
        assertTrue(ArtworkPolicy.isAllowedUrl("https://cdn.example.com/a/b.jpg?x=1"))
        assertTrue(ArtworkPolicy.isAllowedUrl("HTTPS://cdn.example.com:443/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("http://cdn.example.com/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("file:///sdcard/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("content://media/external/1"))
        assertFalse(ArtworkPolicy.isAllowedUrl("ftp://cdn.example.com/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("//cdn.example.com/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("cdn.example.com/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl(null))
        assertFalse(ArtworkPolicy.isAllowedUrl(""))
    }

    @Test
    fun malformedOrSneakyUrlsAreRefused() {
        assertFalse(ArtworkPolicy.isAllowedUrl("https://user:pw@cdn.example.com/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("https://cdn.example.com/a b.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("https://cdn.example.com/a\n.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("https://cdn.example.com:8443/a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("https:///a.jpg"))
        assertFalse(ArtworkPolicy.isAllowedUrl("https://exa mple.com/"))
        assertFalse(ArtworkPolicy.isAllowedUrl("https://cdn.example.com/" + "a".repeat(ArtworkPolicy.MAX_URL_LENGTH)))
    }

    @Test
    fun hostsOnTheDeviceOrPrivateNetworksAreRefused() {
        for (host in listOf(
            "localhost", "foo.localhost", "printer.local", "127.0.0.1", "10.0.0.5", "192.168.1.1", "172.16.0.1",
            "172.31.255.255", "169.254.169.254", "0.0.0.0", "100.64.0.1", "[::1]", "224.0.0.1",
        )) {
            assertFalse(host, ArtworkPolicy.isAllowedUrl("https://$host/a.jpg"))
        }
        assertTrue(ArtworkPolicy.isAllowedUrl("https://172.32.0.1/a.jpg"))
        assertTrue(ArtworkPolicy.isAllowedUrl("https://8.8.8.8/a.jpg"))
    }

    @Test
    fun sizeLimitsOnTheSource() {
        assertTrue(ArtworkPolicy.isAllowedSize(1200, 1200))
        assertFalse(ArtworkPolicy.isAllowedSize(0, 100))
        assertFalse(ArtworkPolicy.isAllowedSize(-1, 100))
        assertFalse(ArtworkPolicy.isAllowedSize(ArtworkPolicy.MAX_SOURCE_DIMENSION + 1, 10))
        assertFalse(ArtworkPolicy.isAllowedSize(30000, 30000))
    }

    @Test
    fun sampleSizeBringsTheLongestSideNearTheTarget() {
        assertEquals(1, ArtworkPolicy.sampleSize(300, 300))
        assertEquals(1, ArtworkPolicy.sampleSize(1000, 900))
        assertEquals(2, ArtworkPolicy.sampleSize(1024, 1024))
        assertEquals(4, ArtworkPolicy.sampleSize(2048, 1000))
        assertEquals(8, ArtworkPolicy.sampleSize(5000, 5000))
    }

    @Test
    fun readLimitedReturnsSmallBodiesAndRefusesHugeOnes() {
        val small = ByteArray(1000) { it.toByte() }
        assertArrayEquals(small, ArtworkPolicy.readLimited(ByteArrayInputStream(small), 1000))
        assertNull(ArtworkPolicy.readLimited(ByteArrayInputStream(ByteArray(1001)), 1000))
        assertNotNull(ArtworkPolicy.readLimited(ByteArrayInputStream(ByteArray(0)), 10))
    }

    @Test
    fun readLimitedStopsReadingAtTheLimitOnAnEndlessStream() {
        var served = 0L
        val endless = object : InputStream() {
            override fun read(): Int = 0.also { served++ }
            override fun read(b: ByteArray, off: Int, len: Int): Int {
                served += len
                return len
            }
        }
        assertNull(ArtworkPolicy.readLimited(endless, 100_000))
        assertTrue("read $served bytes", served < 200_000)
    }

    @Test
    fun redirectsMustStayOnAllowedHttpsUrls() {
        val here = "https://cdn.example.com/a/b.jpg"
        assertEquals("https://cdn.example.com/c.jpg", ArtworkPolicy.redirectTarget(here, "/c.jpg"))
        assertEquals("https://other.example.org/x", ArtworkPolicy.redirectTarget(here, "https://other.example.org/x"))
        assertNull(ArtworkPolicy.redirectTarget(here, "http://cdn.example.com/c.jpg"))
        assertNull(ArtworkPolicy.redirectTarget(here, "https://127.0.0.1/x"))
        assertNull(ArtworkPolicy.redirectTarget(here, "file:///etc/passwd"))
        assertNull(ArtworkPolicy.redirectTarget(here, null))
    }
}
