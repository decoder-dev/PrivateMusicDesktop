package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

data class MixLaunch(val source: JsonObject, val tracks: List<Track>, val nextOffset: Int?)

class CatalogViewModel(
    private val api: RustPlayerApi,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) {
    private val mutableLibrary = MutableStateFlow<List<Track>>(emptyList())
    private val mutableSearch = MutableStateFlow<List<Track>>(emptyList())
    private val mutableMixes = MutableStateFlow<List<MusicMix>>(emptyList())
    private val mutableReleases = MutableStateFlow<List<Album>>(emptyList())
    private val mutableDownloads = MutableStateFlow<List<OfflineTrack>>(emptyList())
    private val mutableLyrics = MutableStateFlow<Lyrics?>(null)
    private val mutableError = MutableStateFlow<String?>(null)
    private val mutableLoading = MutableStateFlow(false)

    val library: StateFlow<List<Track>> = mutableLibrary.asStateFlow()
    val searchResults: StateFlow<List<Track>> = mutableSearch.asStateFlow()
    val mixes: StateFlow<List<MusicMix>> = mutableMixes.asStateFlow()
    val releases: StateFlow<List<Album>> = mutableReleases.asStateFlow()
    val downloads: StateFlow<List<OfflineTrack>> = mutableDownloads.asStateFlow()
    val lyrics: StateFlow<Lyrics?> = mutableLyrics.asStateFlow()
    val error: StateFlow<String?> = mutableError.asStateFlow()
    val loading: StateFlow<Boolean> = mutableLoading.asStateFlow()

    fun start() {
        refreshLibrary()
        refreshMixes()
        refreshDownloads()
    }

    fun refreshLibrary() = run {
        mutableLibrary.value = api.library().items
        mutableSearch.value = api.recommendations()
        mutableReleases.value = api.newReleases()
    }

    fun refreshMixes() = run { mutableMixes.value = api.catalogMixes() }
    fun refreshDownloads() = run { mutableDownloads.value = api.downloads() }

    fun search(query: String) = run {
        mutableSearch.value = if (query.isBlank()) api.recommendations() else api.search(query).items
    }

    fun playMix(mix: MusicMix): MixLaunch {
        var options = mix.options
        var tracks = try {
            api.streamMix(mix.id, options)
        } catch (error: PlayerException) {
            if (options == null || !error.message.orEmpty().contains("error 8", ignoreCase = true) && !error.message.orEmpty().contains("bad request", true)) throw error
            options = null
            api.streamMix(mix.id, null)
        }
        if (tracks.isEmpty() && options != null) {
            options = null
            tracks = api.streamMix(mix.id, null)
        }
        val source = buildJsonObject {
            if (mix.id == "common" && options == null) {
                put("kind", "personal")
                put("options", null)
            } else {
                put("kind", "catalog")
                put("mix_id", mix.id)
                put("options", options)
            }
        }
        return MixLaunch(source, tracks, 400)
    }

    fun download(track: Track) = run {
        api.download(track)
        refreshDownloads()
    }

    fun removeDownload(track: Track) = run {
        api.removeDownload(track.key)
        refreshDownloads()
    }

    fun lyrics(track: Track) = run { mutableLyrics.value = api.lyrics(track) }

    private fun run(action: () -> Unit) {
        scope.launch {
            mutableLoading.value = true
            runCatching(action)
                .onFailure { mutableError.value = it.message }
            mutableLoading.value = false
        }
    }

    fun close() = scope.cancel()
}
