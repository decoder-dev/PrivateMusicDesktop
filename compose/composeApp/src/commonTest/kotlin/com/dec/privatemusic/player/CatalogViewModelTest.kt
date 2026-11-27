package com.dec.privatemusic.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

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
}
