package com.dec.privatemusic.player

import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.json.JsonObject

interface PlayerApi {
    suspend fun state(): PlayerState
    suspend fun playTracks(tracks: List<Track>, index: Int)
    suspend fun toggle()
    suspend fun next()
    suspend fun previous()
    suspend fun seek(seconds: Double)
    suspend fun setVolume(volume: Float)
    suspend fun jump(index: Int)
    suspend fun queueSnapshot(): QueueEvent
    suspend fun playNext(track: Track)
    suspend fun playLast(track: Track)
    suspend fun moveUpcoming(from: Int, to: Int)
    suspend fun removeFromQueue(index: Int)
    suspend fun clearUpcoming()
    fun setShuffle(on: Boolean)
    fun setRepeat(repeat: String)
    fun equalizer(): List<Float>?
    fun setEqualizer(gains: List<Float>?)
    fun sleepTimer(): SleepTimer
    fun setSleepTimer(seconds: Long?, endOfTrack: Boolean)
    fun setVisualizer(on: Boolean)
    suspend fun playRadio(source: JsonObject, tracks: List<Track>, index: Int, nextOffset: Int?)
    fun events(): Flow<String>
    fun close() {}
}

expect fun platformPlayerApi(): PlayerApi
expect fun platformSignIn(session: Session)
expect fun platformSignOut()
expect fun platformSessionState(): SessionState
expect fun platformCatalogApi(): CatalogApi
