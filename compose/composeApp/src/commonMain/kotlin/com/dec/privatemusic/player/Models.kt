package com.dec.privatemusic.player

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.Serializable

@Serializable
data class Track(
    val track_id: Long,
    val owner_id: Long,
    val title: String,
    val artist: String,
    val album_title: String? = null,
    val duration: Int,
    val stream_url: String? = null,
    val artwork_url: String? = null,
    val access_key: String? = null,
    val lyrics_id: Long? = null,
    val is_hq: Boolean = false,
    val track_code: String? = null,
    val artists: List<JsonElement> = emptyList(),
    val album: JsonElement? = null,
) {
    val key: String get() = "${owner_id}_$track_id"
}

@Serializable
data class PlayerState(
    val status: String = "idle",
    val track: Track? = null,
    val position: Double = 0.0,
    val duration: Double = 0.0,
    val volume: Float = 1.0f,
    val shuffle: Boolean = false,
    val repeat: String = "off",
    val queue_index: Int? = null,
    val queue_len: Int = 0,
    val can_next: Boolean = false,
    val radio: String? = null,
    val radio_context: JsonElement? = null,
)

@Serializable
data class QueueEvent(
    val tracks: List<Track> = emptyList(),
    val current: Int? = null,
)

val demoTracks = listOf(
    Track(101, 1, "Reise, Reise", "Rammstein", duration = 250),
    Track(102, 1, "Engel", "Rammstein", duration = 264),
    Track(103, 1, "The Cycle of Songs", "Sabaton", duration = 339),
    Track(104, 1, "Nightside of Siberia", "Powerwolf", duration = 233),
    Track(105, 1, "My Demons", "STARSET", duration = 288),
)
