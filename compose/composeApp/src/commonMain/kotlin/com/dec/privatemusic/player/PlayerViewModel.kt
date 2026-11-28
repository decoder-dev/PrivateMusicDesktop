package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
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
    private val mutableQueue = MutableStateFlow(QueueEvent())
    private val mutableEqualizer = MutableStateFlow<List<Float>?>(null)
    private val mutableTimer = MutableStateFlow(SleepTimer())
    private val mutableSpectrum = MutableStateFlow<List<Float>>(emptyList())
    private val actions = Mutex()
    private var previousVolume = 1f
    private val mutableError = MutableStateFlow<String?>(null)
    private var started = false

    val state: StateFlow<PlayerState> = mutableState.asStateFlow()
    val queue: StateFlow<QueueEvent> = mutableQueue.asStateFlow()
    val equalizer = mutableEqualizer.asStateFlow()
    val sleepTimer = mutableTimer.asStateFlow()
    val spectrum = mutableSpectrum.asStateFlow()
    val error: StateFlow<String?> = mutableError.asStateFlow()

    fun start(onSessionExpired: () -> Unit = {}) {
        if (started) return
        started = true
        scope.launch {
            api.events().collect { raw ->
                runCatching {
                    val element = json.parseToJsonElement(raw)
                    val body = element as? JsonObject ?: return@runCatching
                    when (body["type"]?.jsonPrimitive?.contentOrNull) {
                        "queue" -> mutableQueue.value = json.decodeFromJsonElement(QueueEvent.serializer(), body)
                        "sleep_timer" -> mutableTimer.value = json.decodeFromJsonElement<SleepTimer>(body)
                        "spectrum" -> body["bands"]?.let { mutableSpectrum.value = json.decodeFromJsonElement<List<Float>>(it) }
                        "error" -> mutableError.value = body["message"]?.jsonPrimitive?.contentOrNull
                        "session_expired" -> onSessionExpired()
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
        run {
            mutableQueue.value = api.queueSnapshot()
            mutableEqualizer.value = api.equalizer()
            mutableTimer.value = api.sleepTimer()
        }
    }

    fun play(tracks: List<Track>, index: Int = 0) = run { api.playTracks(tracks, index) }
    fun toggle() = run { api.toggle() }
    fun next() = run { api.next() }
    fun previous() = run { api.previous() }
    fun seek(seconds: Double) = run { api.seek(seconds) }
    fun setVolume(volume: Float) = run { api.setVolume(volume) }
    fun jump(index: Int) = run { api.jump(index) }
    fun toggleShuffle() = run { api.setShuffle(!api.state().shuffle) }
    fun cycleRepeat() = run {
        api.setRepeat(when (api.state().repeat) {
            "off" -> "all"
            "all" -> "one"
            else -> "off"
        })
    }
    fun toggleMute() = run {
        val volume = api.state().volume
        if (volume > 0f) previousVolume = volume
        api.setVolume(if (volume > 0f) 0f else previousVolume)
    }
    fun playNext(track: Track) = run { api.playNext(track) }
    fun playLast(track: Track) = run { api.playLast(track) }
    fun moveUpcoming(index: Int, to: Int) = run { api.moveUpcoming(index, to) }
    fun removeFromQueue(index: Int) = run { api.removeFromQueue(index) }
    fun clearUpcoming() = run { api.clearUpcoming() }
    fun setEqualizer(gains: List<Float>?) = run {
        api.setEqualizer(gains)
        mutableEqualizer.value = api.equalizer()
    }
    fun refreshSleepTimer() {
        scope.launch {
            actions.withLock {
                runCatching { api.sleepTimer() }.onSuccess { mutableTimer.value = it }
            }
        }
    }
    fun setSleepTimer(seconds: Long?, endOfTrack: Boolean = false) = run {
        api.setSleepTimer(seconds, endOfTrack)
        mutableTimer.value = api.sleepTimer()
    }
    fun setVisualizer(on: Boolean) = run {
        api.setVisualizer(on)
        if (!on) mutableSpectrum.value = emptyList()
    }
    fun playRadio(source: kotlinx.serialization.json.JsonObject, tracks: List<Track>, index: Int, nextOffset: Int?) = run { api.playRadio(source, tracks, index, nextOffset) }

    private fun run(action: suspend () -> Unit) {
        scope.launch {
            actions.withLock {
                mutableError.value = null
                runCatching {
                    action()
                    api.state()
                }
                    .onSuccess { mutableState.value = it }
                    .onFailure { mutableError.value = it.message }
            }
        }
    }

    fun close() {
        scope.cancel()
        api.close()
    }
}
