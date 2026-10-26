package com.dec.privatemusic.mediasession

import java.net.HttpURLConnection
import java.net.URL
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ArtworkLoaderTest {
    @Test
    fun rapidSkipsDoNotDownloadQueuedObsoleteCovers() {
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        val completed = CountDownLatch(1)
        val requests = Collections.synchronizedList(mutableListOf<String>())
        val callbacks = Collections.synchronizedList(mutableListOf<String>())
        val worker = Executors.newSingleThreadExecutor()
        val loader = ArtworkLoader(worker) { url ->
            requests.add(url.path)
            response(url) {
                if (url.path == "/first") {
                    started.countDown()
                    assertTrue(release.await(5, TimeUnit.SECONDS))
                }
                HttpURLConnection.HTTP_NOT_FOUND
            }
        }
        try {
            loader.load("https://cdn.example.com/first") { callbacks.add("first") }
            assertTrue(started.await(5, TimeUnit.SECONDS))
            repeat(32) { i ->
                loader.load("https://cdn.example.com/skipped-$i") { callbacks.add("skipped-$i") }
            }
            loader.load("https://cdn.example.com/current") {
                callbacks.add("current")
                completed.countDown()
            }
            release.countDown()
            assertTrue(completed.await(5, TimeUnit.SECONDS))
            assertEquals(listOf("/first", "/current"), requests.toList())
            assertEquals(listOf("current"), callbacks.toList())
        } finally {
            release.countDown()
            worker.shutdownNow()
        }
    }

    @Test
    fun cancelSkipsWorkThatHasNotStarted() {
        val release = CountDownLatch(1)
        val requests = Collections.synchronizedList(mutableListOf<String>())
        val worker = Executors.newSingleThreadExecutor()
        worker.execute { release.await(5, TimeUnit.SECONDS) }
        val loader = ArtworkLoader(worker) { url ->
            requests.add(url.path)
            response(url) { HttpURLConnection.HTTP_NOT_FOUND }
        }
        try {
            loader.load("https://cdn.example.com/cancelled") { requests.add("callback") }
            loader.cancel()
            release.countDown()
            worker.submit {}.get(5, TimeUnit.SECONDS)
            assertTrue(requests.isEmpty())
        } finally {
            release.countDown()
            worker.shutdownNow()
        }
    }

    @Test
    fun obsoleteRequestDoesNotFollowRedirects() {
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        val completed = CountDownLatch(1)
        val requests = Collections.synchronizedList(mutableListOf<String>())
        val worker = Executors.newSingleThreadExecutor()
        val loader = ArtworkLoader(worker) { url ->
            requests.add(url.path)
            response(url, location = "/obsolete-redirect") {
                if (url.path == "/first") {
                    started.countDown()
                    assertTrue(release.await(5, TimeUnit.SECONDS))
                    HttpURLConnection.HTTP_MOVED_TEMP
                } else {
                    HttpURLConnection.HTTP_NOT_FOUND
                }
            }
        }
        try {
            loader.load("https://cdn.example.com/first") {}
            assertTrue(started.await(5, TimeUnit.SECONDS))
            loader.load("https://cdn.example.com/current") { completed.countDown() }
            release.countDown()
            assertTrue(completed.await(5, TimeUnit.SECONDS))
            assertEquals(listOf("/first", "/current"), requests.toList())
        } finally {
            release.countDown()
            worker.shutdownNow()
        }
    }

    // Error responses finish before bitmap decoding, so these tests need no Android runtime.
    private fun response(url: URL, location: String? = null, code: () -> Int) =
        object : HttpURLConnection(url) {
            override fun connect() {}
            override fun disconnect() {}
            override fun usingProxy() = false
            override fun getResponseCode() = code()
            override fun getHeaderField(name: String?) = if (name == "Location") location else null
        }
}
