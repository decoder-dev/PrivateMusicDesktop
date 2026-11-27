package com.dec.privatemusic.player

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
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

    override fun events(): Flow<String> = flow {
        while (currentCoroutineContext().isActive) {
            val raw = transport.poll()
            if (raw != null) emit(raw) else delay(25)
        }
    }

    override fun close() = transport.free()
}

class PlayerException(message: String) : IllegalStateException(message)
