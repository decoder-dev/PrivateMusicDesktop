package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

data class MixLaunch(val source: JsonObject, val tracks: List<Track>, val nextOffset: Int?)
data class TrackLyrics(val trackKey: String, val lyrics: Lyrics? = null, val error: String? = null)

class CatalogViewModel(
    private val api: CatalogApi,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) {
    val isDemo: Boolean get() = api.isDemo

    private val mutableLibrary = MutableStateFlow<List<Track>>(emptyList())
    private val mutableFavoriteCopies = MutableStateFlow<Map<String, Track>>(emptyMap())
    private val mutableCanFavorite = MutableStateFlow(false)
    private val favoriteOperation = Mutex()
    private val mutableRecommendations = MutableStateFlow<List<Track>>(emptyList())
    private val mutableSearch = MutableStateFlow<List<Track>>(emptyList())
    private val mutableMixes = MutableStateFlow<List<MusicMix>>(emptyList())
    private val mutableReleases = MutableStateFlow<List<Album>>(emptyList())
    private val mutableDownloads = MutableStateFlow<List<OfflineTrack>>(emptyList())
    private val mutableLyrics = MutableStateFlow<TrackLyrics?>(null)
    private val mutableError = MutableStateFlow<String?>(null)
    private val mutableLoading = MutableStateFlow(false)
    private val mutableSearching = MutableStateFlow(false)
    private var searchJob: Job? = null
    private var lyricsJob: Job? = null

    val library: StateFlow<List<Track>> = mutableLibrary.asStateFlow()
    val favoriteCopies = mutableFavoriteCopies.asStateFlow()
    val canFavorite = mutableCanFavorite.asStateFlow()
    val recommendations: StateFlow<List<Track>> = mutableRecommendations.asStateFlow()
    val searchResults: StateFlow<List<Track>> = mutableSearch.asStateFlow()
    val mixes: StateFlow<List<MusicMix>> = mutableMixes.asStateFlow()
    val releases: StateFlow<List<Album>> = mutableReleases.asStateFlow()
    val downloads: StateFlow<List<OfflineTrack>> = mutableDownloads.asStateFlow()
    val lyrics: StateFlow<TrackLyrics?> = mutableLyrics.asStateFlow()
    val error: StateFlow<String?> = mutableError.asStateFlow()
    val loading: StateFlow<Boolean> = mutableLoading.asStateFlow()
    val searching: StateFlow<Boolean> = mutableSearching.asStateFlow()

    fun start() {
        run {
            mutableLibrary.value = api.library().items
            mutableCanFavorite.value = true
        }
        run { mutableRecommendations.value = api.recommendations() }
        run { mutableReleases.value = api.newReleases() }
        refreshMixes()
        refreshDownloads()
    }

    fun refreshMixes() = run { mutableMixes.value = api.catalogMixes() }
    fun refreshDownloads() = run { mutableDownloads.value = api.downloads() }

    fun search(query: String) {
        searchJob?.cancel()
        mutableSearching.value = true
        mutableSearch.value = emptyList()
        mutableError.value = null
        searchJob = scope.launch {
            try {
                val result = runCatching {
                    if (query.isBlank()) api.recommendations() else api.search(query).items
                }
                ensureActive()
                result.onSuccess { mutableSearch.value = it }
                    .onFailure { mutableError.value = it.message }
            } finally {
                if (isActive) mutableSearching.value = false
            }
        }
    }

    fun playMix(mix: MusicMix, onReady: (MixLaunch) -> Unit) {
        scope.launch {
            mutableLoading.value = true
            mutableError.value = null
            try {
                val result = runCatching { loadMix(mix) }
                ensureActive()
                result.onSuccess(onReady).onFailure { mutableError.value = it.message }
            } finally {
                mutableLoading.value = false
            }
        }
    }

    private fun loadMix(mix: MusicMix): MixLaunch {
        var options = mix.options
        var tracks = try {
            api.streamMix(mix.id, options)
        } catch (error: PlayerException) {
            val message = error.message.orEmpty()
            val optionsRejected = message.equals("Invalid request.", ignoreCase = true) ||
                message.contains("error 8:", ignoreCase = true) ||
                message.contains("bad request", ignoreCase = true)
            if (options == null || !optionsRejected) throw error
            options = null
            api.streamMix(mix.id, null)
        }
        if (tracks.isEmpty() && options != null) {
            options = null
            tracks = api.streamMix(mix.id, null)
        }
        if (tracks.isEmpty()) throw PlayerException("В этом миксе пока нет треков.")
        val source = buildJsonObject {
            if (mix.id == "common" && options == null) {
                put("kind", "personal")
            } else {
                put("kind", "catalog")
                put("mix_id", mix.id)
            }
            put("options", options)
        }
        return MixLaunch(source, tracks, 400)
    }

    fun toggleFavorite(track: Track) = run {
        if (!mutableCanFavorite.value || !favoriteOperation.tryLock()) return@run
        mutableCanFavorite.value = false
        mutableError.value = null
        try {
            val favorite = mutableLibrary.value.firstOrNull { it.key == track.key }
                ?: mutableFavoriteCopies.value[track.key]
            if (favorite != null) {
                api.removeFromLibrary(favorite)
                mutableLibrary.update { tracks -> tracks.filterNot { it.key == favorite.key } }
                mutableFavoriteCopies.update { copies -> copies.filterValues { it.key != favorite.key } }
            } else {
                val added = api.addToLibrary(track)
                mutableLibrary.update { tracks -> listOf(added) + tracks.filterNot { it.key == added.key } }
                if (added.key != track.key) mutableFavoriteCopies.update { it + (track.key to added) }
            }
        } finally {
            mutableCanFavorite.value = true
            favoriteOperation.unlock()
        }
    }

    fun download(track: Track) = run {
        api.download(track)
        refreshDownloads()
    }

    fun removeDownload(track: Track) = run {
        api.removeDownload(track.key)
        refreshDownloads()
    }

    fun lyrics(track: Track) {
        lyricsJob?.cancel()
        mutableLyrics.value = null
        lyricsJob = scope.launch {
            val result = runCatching { api.lyrics(track) }
            ensureActive()
            mutableLyrics.value = result.fold(
                onSuccess = { TrackLyrics(track.key, lyrics = it) },
                onFailure = { TrackLyrics(track.key, error = it.message ?: "Не удалось загрузить текст") },
            )
        }
    }

    private fun run(action: () -> Unit) {
        scope.launch {
            runCatching(action)
                .onFailure { mutableError.value = it.message }
        }
    }

    fun close() = scope.cancel()
}
