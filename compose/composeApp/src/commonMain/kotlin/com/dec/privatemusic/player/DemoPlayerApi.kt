package com.dec.privatemusic.player

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlin.time.TimeSource
import kotlin.time.Duration.Companion.seconds

class DemoPlayerApi : PlayerApi {
    private val json = Json { ignoreUnknownKeys = true }
    private var player = PlayerState()
    private var queue = emptyList<Track>()
    private var equalizerGains: List<Float>? = null
    private var timer = SleepTimer()
    private var deadline: TimeSource.Monotonic.ValueTimeMark? = null
    private var playing = false

    override suspend fun state(): PlayerState = player

    override suspend fun playTracks(tracks: List<Track>, index: Int) {
        queue = tracks
        player = player.copy(
            status = "playing",
            track = tracks[index],
            position = 0.0,
            duration = tracks[index].duration.toDouble(),
            queue_index = index,
            queue_len = tracks.size,
            can_next = player.repeat == "all" || index + 1 < tracks.size,
        )
        playing = true
    }

    override suspend fun toggle() {
        if (player.track == null) playTracks(queue.ifEmpty { demoTracks }, 0)
        else {
            playing = !playing
            player = player.copy(status = if (playing) "playing" else "paused")
        }
    }

    override suspend fun next() {
        val next = (player.queue_index ?: -1) + 1
        jump(if (next == queue.size && player.repeat == "all") 0 else next)
    }
    override suspend fun previous() = jump((player.queue_index ?: 0) - 1)

    override suspend fun seek(seconds: Double) {
        player = player.copy(position = seconds.coerceIn(0.0, player.duration))
    }

    override suspend fun setVolume(volume: Float) {
        player = player.copy(volume = volume.coerceIn(0f, 1f))
    }

    override suspend fun jump(index: Int) {
        if (index !in queue.indices) return
        playTracks(queue, index)
    }

    override suspend fun queueSnapshot() = QueueEvent(queue, player.queue_index)

    override suspend fun playNext(track: Track) {
        if (queue.isEmpty()) {
            playTracks(listOf(track), 0)
            return
        }
        val index = (player.queue_index ?: -1) + 1
        replaceQueue(queue.toMutableList().apply { add(index, track) })
    }

    override suspend fun playLast(track: Track) {
        if (queue.isEmpty()) playTracks(listOf(track), 0) else replaceQueue(queue + track)
    }

    override suspend fun moveUpcoming(from: Int, to: Int) {
        val firstUpcoming = (player.queue_index ?: -1) + 1
        if (from in firstUpcoming until queue.size && to in firstUpcoming until queue.size) {
            replaceQueue(queue.toMutableList().apply { add(to, removeAt(from)) })
        }
    }

    override suspend fun removeFromQueue(index: Int) {
        if (index !in queue.indices) return
        val current = player.queue_index ?: 0
        val updated = queue.toMutableList().apply { removeAt(index) }
        when {
            updated.isEmpty() || (index == current && index == updated.size) -> {
                playing = false
                player = player.copy(status = "idle", track = null, position = 0.0, duration = 0.0)
                replaceQueue(updated, null)
            }
            index == current -> playTracks(updated, current)
            else -> replaceQueue(updated, if (index < current) current - 1 else current)
        }
    }

    override suspend fun clearUpcoming() = replaceQueue(queue.take((player.queue_index ?: -1) + 1))

    private fun replaceQueue(tracks: List<Track>, index: Int? = player.queue_index) {
        queue = tracks
        player = player.copy(
            queue_index = index,
            queue_len = queue.size,
            can_next = queue.isNotEmpty() && (player.repeat == "all" || (index ?: -1) + 1 < queue.size),
        )
    }

    override fun setShuffle(on: Boolean) {
        if (on == player.shuffle) return
        player = player.copy(shuffle = on)
        if (on) {
            val firstUpcoming = (player.queue_index ?: -1) + 1
            replaceQueue(queue.take(firstUpcoming) + queue.drop(firstUpcoming).shuffled())
        }
    }

    override fun setRepeat(repeat: String) {
        player = player.copy(repeat = repeat)
        replaceQueue(queue)
    }

    override fun equalizer(): List<Float>? = equalizerGains
    override fun setEqualizer(gains: List<Float>?) {
        equalizerGains = gains
    }

    override fun sleepTimer() = deadline?.let {
        SleepTimer(remaining = (-it.elapsedNow()).inWholeSeconds.coerceAtLeast(0))
    } ?: timer

    override fun setSleepTimer(seconds: Long?, endOfTrack: Boolean) {
        timer = SleepTimer(seconds, endOfTrack)
        deadline = seconds?.let { TimeSource.Monotonic.markNow() + it.seconds }
    }

    override fun setVisualizer(on: Boolean) = Unit

    override suspend fun playRadio(source: kotlinx.serialization.json.JsonObject, tracks: List<Track>, index: Int, nextOffset: Int?) = playTracks(tracks, index)

    override fun events(): Flow<String> = flow {
        var lastQueue: QueueEvent? = null
        while (true) {
            delay(250)
            if (playing && player.track != null) {
                val next = (player.position + 0.25).coerceAtMost(player.duration)
                player = player.copy(position = next)
                if (next >= player.duration) {
                    when {
                        timer.end_of_track -> {
                            setSleepTimer(null, false)
                            toggle()
                        }
                        player.repeat == "one" -> seek(0.0)
                        player.can_next -> next()
                        else -> toggle()
                    }
                }
            }
            if (deadline?.hasPassedNow() == true) {
                setSleepTimer(null, false)
                if (playing) toggle()
            }
            val snapshot = queueSnapshot()
            if (snapshot != lastQueue) {
                lastQueue = snapshot
                val payload = json.encodeToJsonElement(QueueEvent.serializer(), snapshot).jsonObject
                emit(JsonObject(payload + ("type" to JsonPrimitive("queue"))).toString())
            }
            emit(json.encodeToString(PlayerState.serializer(), player))
        }
    }
}
