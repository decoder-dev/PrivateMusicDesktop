package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

class PlayerViewModel(
    private val api: PlayerApi,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) {
    private val json = Json { ignoreUnknownKeys = true }
    private val mutableState = MutableStateFlow(PlayerState())
    private val mutableQueue = MutableStateFlow<List<Track>>(emptyList())
    private val mutableError = MutableStateFlow<String?>(null)
    private var started = false

    val state: StateFlow<PlayerState> = mutableState.asStateFlow()
    val queue: StateFlow<List<Track>> = mutableQueue.asStateFlow()
    val error: StateFlow<String?> = mutableError.asStateFlow()

    fun start() {
        if (started) return
        started = true
        scope.launch {
            api.events().collect { raw ->
                runCatching {
                    val element = json.parseToJsonElement(raw)
                    val body = element as? JsonObject ?: return@runCatching
                    when (body["type"]?.jsonPrimitive?.contentOrNull) {
                        "queue" -> mutableQueue.value = json.decodeFromJsonElement(QueueEvent.serializer(), body).tracks
                        "error" -> mutableError.value = body["message"]?.jsonPrimitive?.contentOrNull
                        else -> {
                            if (body.isPlayerStateEvent()) {
                                mutableState.value = json.decodeFromJsonElement(PlayerState.serializer(), body)
                            }
                        }
                    }
                    Unit
                }.onFailure { mutableError.value = it.message }
            }
        }
        refresh()
    }

    private fun refresh() = scope.launch {
        runCatching { api.state() }
            .onSuccess { mutableState.value = it }
            .onFailure { mutableError.value = it.message }
    }

    fun play(tracks: List<Track>, index: Int = 0) = run { api.playTracks(tracks, index) }
    fun toggle() = run { api.toggle() }
    fun next() = run { api.next() }
    fun previous() = run { api.previous() }
    fun seek(seconds: Double) = run { api.seek(seconds) }
    fun setVolume(volume: Float) = run { api.setVolume(volume) }
    fun jump(index: Int) = run { api.jump(index) }
    fun playRadio(source: kotlinx.serialization.json.JsonObject, tracks: List<Track>, index: Int, nextOffset: Int?) = run { api.playRadio(source, tracks, index, nextOffset) }

    private fun run(action: suspend () -> Unit) {
        scope.launch {
            mutableError.value = null
            runCatching {
                action()
                api.state()
            }
                .onSuccess { mutableState.value = it }
                .onFailure { mutableError.value = it.message }
        }
    }

    fun close() {
        scope.cancel()
        api.close()
    }
}
