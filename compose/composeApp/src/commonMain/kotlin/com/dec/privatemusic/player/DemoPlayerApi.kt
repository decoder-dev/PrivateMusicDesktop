package com.dec.privatemusic.player

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

class DemoPlayerApi : PlayerApi {
    private val json = Json { ignoreUnknownKeys = true }
    private var player = PlayerState()
    private var queue = demoTracks
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
            can_next = index + 1 < tracks.size,
        )
        playing = true
    }

    override suspend fun toggle() {
        if (player.track == null) playTracks(queue, 0)
        else {
            playing = !playing
            player = player.copy(status = if (playing) "playing" else "paused")
        }
    }

    override suspend fun next() = jump((player.queue_index ?: -1) + 1)
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

    override fun events(): Flow<String> = flow {
        while (true) {
            delay(250)
            if (playing && player.track != null) {
                val next = (player.position + 0.25).coerceAtMost(player.duration)
                player = player.copy(position = next)
                if (next >= player.duration) next()
            }
            emit(json.encodeToString(PlayerState.serializer(), player))
        }
    }
}
