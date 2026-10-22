package com.dec.privatemusic.mediasession

import org.json.JSONObject

/** Parsing of the JSON the Rust side sends (`Update` in `lib.rs`). Tolerant: bad input never throws. */
object Wire {
    data class Update(
        val track: TrackInfo?,
        val clearTrack: Boolean,
        val playback: PlaybackInfo?,
    )

    fun parseUpdate(json: JSONObject): Update {
        val track = json.optJSONObject("metadata")?.let { m ->
            TrackInfo(
                title = m.optString("title", "").take(MAX_TEXT),
                artist = m.optString("artist", "").take(MAX_TEXT),
                album = if (m.isNull("album")) null else m.optString("album", "").take(MAX_TEXT).ifEmpty { null },
                artworkUrl = if (m.isNull("artwork_url")) null else m.optString("artwork_url", "").ifEmpty { null },
                durationMs = secondsToMs(m.optDouble("duration_secs", 0.0)),
            )
        }
        val playback = json.optJSONObject("playback")?.let { p ->
            PlaybackInfo(
                kind = PlaybackKind.fromWire(p.optString("state", "")),
                positionMs = secondsToMs(p.optDouble("position_secs", 0.0)),
                durationMs = secondsToMs(p.optDouble("duration_secs", 0.0)),
                rate = p.optDouble("rate", 1.0).let { if (it.isFinite() && it > 0.0) it.coerceAtMost(4.0).toFloat() else 1f },
            )
        }
        return Update(track, json.optBoolean("clear_metadata", false), playback)
    }

    fun toJson(event: ActionEvent): JSONObject {
        val json = JSONObject().put("action", event.name)
        if (event.value != null) json.put("value", event.value)
        return json
    }

    fun secondsToMs(seconds: Double): Long =
        if (seconds.isFinite() && seconds > 0.0) (seconds * 1000.0).toLong().coerceAtMost(MAX_MS) else 0L

    private const val MAX_TEXT = 512
    private const val MAX_MS = 7L * 24 * 3600 * 1000
}
