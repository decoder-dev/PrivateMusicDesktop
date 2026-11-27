package com.dec.privatemusic.player

import kotlinx.serialization.Serializable

@Serializable
data class Session(
    val access_token: String,
    val user_id: Long? = null,
    val expires_at: Long? = null,
    val api_user_agent: String,
    val refresh_cookie: String,
    val web_user_agent: String,
)

@Serializable
data class Profile(
    val id: Long? = null,
    val first_name: String = "",
    val last_name: String = "",
)

@Serializable
data class SessionState(
    val signed_in: Boolean = false,
    val profile: Profile? = null,
    val persistent: Boolean = true,
)

@Serializable
data class MusicMix(
    val id: String,
    val title: String,
    val subtitle: String = "",
    val artwork_url: String? = null,
    val section_title: String? = null,
    val options: String? = null,
)

@Serializable
data class TrackPage(
    val items: List<Track> = emptyList(),
    val total: Int = 0,
    val next_offset: Int? = null,
)

@Serializable
data class OfflineTrack(
    val track: Track,
    val file: String,
    val bytes: Long,
    val added: Long,
)

@Serializable
data class ArtistRef(
    val id: String,
    val name: String,
)

@Serializable
data class Album(
    val album_id: Long,
    val owner_id: Long,
    val title: String,
    val description: String? = null,
    val count: Int = 0,
    val artwork_url: String? = null,
    val access_key: String? = null,
    val artists: List<ArtistRef> = emptyList(),
    val year: Int? = null,
    val is_followed: Boolean = false,
    val follow_hash: String? = null,
)

@Serializable
data class LyricsLine(
    val time: Double,
    val text: String,
)

@Serializable
data class Lyrics(
    val source: String,
    val text: String,
    val lines: List<LyricsLine> = emptyList(),
)
