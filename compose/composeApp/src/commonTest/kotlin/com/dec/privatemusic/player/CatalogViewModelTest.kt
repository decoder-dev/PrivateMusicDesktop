package com.dec.privatemusic.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive

class CatalogViewModelTest {
    @Test
    fun downloadAndRemoveUpdateOfflineLibrary() {
        val viewModel = CatalogViewModel(DemoCatalogApi(), CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        val track = demoTracks.first()
        viewModel.download(track)
        assertEquals(listOf(track.key), viewModel.downloads.value.map { it.track.key })
        viewModel.removeDownload(track)
        assertTrue(viewModel.downloads.value.isEmpty())
        viewModel.close()
    }

    @Test
    fun blankSearchUsesRecommendations() {
        val viewModel = CatalogViewModel(DemoCatalogApi(), CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        viewModel.search("")
        assertEquals(demoTracks, viewModel.searchResults.value)
        viewModel.search("engel")
        assertEquals(listOf(demoTracks[1]), viewModel.searchResults.value)
        viewModel.close()
    }

    @Test
    fun libraryFailureAndSearchDoNotReplaceHomeRecommendations() {
        val api = object : CatalogApi by DemoCatalogApi() {
            override fun library(offset: Int, count: Int): TrackPage = throw PlayerException("Library unavailable")
        }
        val viewModel = CatalogViewModel(api, CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        try {
            viewModel.start()
            assertEquals("Library unavailable", viewModel.error.value)
            assertEquals(demoTracks, viewModel.recommendations.value)
            assertTrue(viewModel.mixes.value.isNotEmpty())
            assertTrue(viewModel.releases.value.isNotEmpty())
            viewModel.search("engel")
            assertEquals(listOf(demoTracks[1]), viewModel.searchResults.value)
            assertEquals(demoTracks, viewModel.recommendations.value)
        } finally {
            viewModel.close()
        }
    }

    @Test
    fun rejectedMixOptionsRetryAndDoNotCarryOverToTheQueue() {
        val attempts = mutableListOf<String?>()
        val api = object : CatalogApi by DemoCatalogApi() {
            override fun streamMix(id: String, options: String?, offset: Int): List<Track> {
                attempts.add(options)
                if (options != null) throw PlayerException("Invalid request.")
                return demoTracks
            }
        }
        val viewModel = CatalogViewModel(api, CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        try {
            var launch: MixLaunch? = null
            viewModel.playMix(MusicMix("common", "Мой микс", options = "{}")) { launch = it }
            val result = assertNotNull(launch)
            assertEquals(listOf("{}", null), attempts)
            assertEquals(JsonPrimitive("personal"), result.source["kind"])
            assertEquals(JsonNull, result.source["options"])
            assertEquals(demoTracks, result.tracks)
            assertNull(viewModel.error.value)
        } finally {
            viewModel.close()
        }
    }

    @Test
    fun emptyMixDoesNotStartAnEmptyQueue() {
        val api = object : CatalogApi by DemoCatalogApi() {
            override fun streamMix(id: String, options: String?, offset: Int): List<Track> = emptyList()
        }
        val viewModel = CatalogViewModel(api, CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        try {
            viewModel.playMix(MusicMix("daily", "Микс дня")) { error("An empty mix must not replace the current queue") }
            assertNotNull(viewModel.error.value)
        } finally {
            viewModel.close()
        }
    }
}
