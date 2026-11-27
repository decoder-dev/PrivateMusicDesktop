package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.double
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

interface RustTransport {
    fun command(raw: String): String
    fun poll(): String?
    fun free()
}

class RustPlayerApi(
    private val transport: RustTransport,
    private val json: Json = Json { ignoreUnknownKeys = true },
) : PlayerApi {
    override suspend fun state(): PlayerState = decode(command(buildJsonObject { put("command", "player_state") }))

    override suspend fun playTracks(tracks: List<Track>, index: Int) = unitCommand(buildJsonObject {
        put("command", "play_tracks")
        putJsonArray("tracks") { tracks.forEach { add(json.encodeToJsonElement(Track.serializer(), it)) } }
        put("index", index)
    })

    override suspend fun toggle() = unitCommand(buildJsonObject { put("command", "toggle") })
    override suspend fun next() = unitCommand(buildJsonObject { put("command", "next") })
    override suspend fun previous() = unitCommand(buildJsonObject { put("command", "previous") })
    override suspend fun seek(seconds: Double) = unitCommand(buildJsonObject {
        put("command", "seek")
        put("seconds", seconds)
    })
    override suspend fun setVolume(volume: Float) = unitCommand(buildJsonObject {
        put("command", "set_volume")
        put("volume", volume)
    })
    override suspend fun jump(index: Int) = unitCommand(buildJsonObject {
        put("command", "jump")
        put("index", index)
    })

    private fun unitCommand(command: JsonObject) {
        val response = command(command)
        val error = response["error"]?.jsonPrimitive?.content
        if (error != null) throw PlayerException(error)
    }

    private fun command(command: JsonObject): JsonObject {
        val response = json.parseToJsonElement(transport.command(command.toString()))
        return response as? JsonObject ?: throw PlayerException("Invalid engine response")
    }

    private inline fun <reified T> decode(command: JsonObject): T {
        val response = command(command)
        response["error"]?.jsonPrimitive?.content?.let { throw PlayerException(it) }
        return json.decodeFromJsonElement(response)
    }

    fun signIn(session: Session) = unitCommand(buildJsonObject { put("command", "sign_in"); put("session", json.encodeToJsonElement(session)) })
    fun signOut() = unitCommand(buildJsonObject { put("command", "sign_out") })
    fun sessionState(): SessionState = decode(buildJsonObject { put("command", "session_state") })
    fun library(offset: Int = 0, count: Int = 200): TrackPage = decode(buildJsonObject { put("command", "library"); put("offset", offset); put("count", count) })
    fun search(query: String, offset: Int = 0, count: Int = 100): TrackPage = decode(buildJsonObject { put("command", "search"); put("query", query); put("offset", offset); put("count", count) })
    fun catalogMixes(): List<MusicMix> = decode(buildJsonObject { put("command", "catalog_mixes") })
    fun streamMix(id: String, options: String? = null, offset: Int = 0): List<Track> = decode(buildJsonObject { put("command", "stream_mix"); put("mix_id", id); put("options", options); put("offset", offset) })
    override suspend fun playRadio(source: JsonObject, tracks: List<Track>, index: Int, nextOffset: Int?) = unitCommand(buildJsonObject { put("command", "play_radio"); put("source", source); putJsonArray("tracks") { tracks.forEach { add(json.encodeToJsonElement(Track.serializer(), it)) } }; put("index", index); put("next_offset", nextOffset) })
    fun recommendations(): List<Track> = decode(buildJsonObject { put("command", "recommendations") })
    fun newReleases(): List<Album> = decode(buildJsonObject { put("command", "new_releases") })
    fun downloads(): List<OfflineTrack> = decode(buildJsonObject { put("command", "downloads") })
    fun download(track: Track): OfflineTrack = decode(buildJsonObject { put("command", "download"); put("track", json.encodeToJsonElement(track)) })
    fun removeDownload(trackId: String): Boolean = decode(buildJsonObject { put("command", "remove_download"); put("track_id", trackId) })
    fun lyrics(track: Track): Lyrics? = decode(buildJsonObject { put("command", "lyrics"); put("track", json.encodeToJsonElement(track)) })
    fun addToLibrary(track: Track): Track = decode(buildJsonObject { put("command", "add_to_library"); put("track", json.encodeToJsonElement(track)) })
    fun removeFromLibrary(track: Track) = unitCommand(buildJsonObject { put("command", "remove_from_library"); put("track", json.encodeToJsonElement(track)) })
    fun setShuffle(on: Boolean) = unitCommand(buildJsonObject { put("command", "set_shuffle"); put("on", on) })
    fun setRepeat(repeat: String) = unitCommand(buildJsonObject { put("command", "set_repeat"); put("repeat", repeat) })
    fun setEqualizer(gains: List<Float>) = unitCommand(buildJsonObject { put("command", "set_equalizer"); putJsonArray("gains") { gains.forEach { add(it) } } })
    fun outputDevices(): List<String> = decode(buildJsonObject { put("command", "output_devices") })
    fun setOutputDevice(name: String?) = unitCommand(buildJsonObject { put("command", "set_output_device"); put("name", name) })

    private val eventScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val eventStream = MutableSharedFlow<String>(extraBufferCapacity = 64)
    private val eventJob by lazy {
        eventScope.launch {
            while (isActive) {
                val raw = transport.poll()
                if (raw != null) eventStream.emit(raw) else delay(25)
            }
        }
    }

    override fun events(): Flow<String> {
        eventJob // Start one native polling loop shared by every UI and media-session collector.
        return eventStream.asSharedFlow()
    }

    override fun close() {
        eventScope.cancel()
        transport.free()
    }
}

class PlayerException(message: String) : IllegalStateException(message)
