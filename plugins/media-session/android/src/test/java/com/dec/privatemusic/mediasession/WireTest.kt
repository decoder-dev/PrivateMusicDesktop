package com.dec.privatemusic.mediasession

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WireTest {
    @Test
    fun parsesTheUpdateRustSends() {
        val json = JSONObject(
            """{"metadata":{"title":"T","artist":"A","album":"Al","artwork_url":"https://c.example/x.jpg","duration_secs":200.5},
               "clear_metadata":false,
               "playback":{"state":"buffering","position_secs":12.25,"duration_secs":200.5,"rate":1.0}}""",
        )
        val u = Wire.parseUpdate(json)
        assertEquals(TrackInfo("T", "A", "Al", "https://c.example/x.jpg", 200_500), u.track)
        assertFalse(u.clearTrack)
        assertEquals(PlaybackInfo(PlaybackKind.BUFFERING, 12_250, 200_500, 1f), u.playback)
    }

    @Test
    fun nullsAndMissingPartsMeanUnchanged() {
        val u = Wire.parseUpdate(JSONObject("""{"metadata":null,"clear_metadata":true,"playback":null}"""))
        assertNull(u.track)
        assertTrue(u.clearTrack)
        assertNull(u.playback)
        val track = Wire.parseUpdate(JSONObject("""{"metadata":{"title":"T","artist":"A","album":null,"artwork_url":null,"duration_secs":0}}""")).track!!
        assertNull(track.album)
        assertNull(track.artworkUrl)
    }

    @Test
    fun badValuesAreTamedNotThrown() {
        val p = Wire.parseUpdate(JSONObject("""{"playback":{"state":"???","position_secs":-4,"duration_secs":1e300,"rate":-1}}""")).playback!!
        assertEquals(PlaybackKind.STOPPED, p.kind)
        assertEquals(0, p.positionMs)
        assertTrue(p.durationMs in 1..(7L * 24 * 3600 * 1000))
        assertEquals(1f, p.rate, 0f)
    }

    @Test
    fun longTextIsTruncated() {
        val t = Wire.parseUpdate(JSONObject("""{"metadata":{"title":"${"x".repeat(5000)}","artist":"A","duration_secs":1}}""")).track!!
        assertEquals(512, t.title.length)
    }

    @Test
    fun actionsUseTheSharedJsonShape() {
        assertEquals("""{"action":"play"}""", Wire.toJson(ActionEvent.PLAY).toString())
        val pause = Wire.toJson(ActionEvent.PAUSE.copy(source = "zero_volume"))
        assertEquals("pause", pause.getString("action"))
        assertEquals("zero_volume", pause.getString("source"))
        val seek = Wire.toJson(ActionEvent.seekTo(42.5))
        assertEquals("seek_to", seek.getString("action"))
        assertEquals(42.5, seek.getDouble("value"), 0.0)
        val duck = Wire.toJson(ActionEvent.duck(true))
        assertEquals("duck", duck.getString("action"))
        assertTrue(duck.getBoolean("value"))
    }

    @Test
    fun playbackKindsRoundTripTheWireNames() {
        for (kind in PlaybackKind.entries) assertEquals(kind, PlaybackKind.fromWire(kind.wire))
        assertEquals(PlaybackKind.STOPPED, PlaybackKind.fromWire(null))
    }
    @Test fun explicitNextAvailabilityIsPreservedAndLegacyPayloadsStillWork() {
        val end = Wire.parseUpdate(JSONObject("""{"playback":{"state":"playing","can_next":false}}""")).playback!!
        assertFalse(end.canNext)
        val next = Wire.parseUpdate(JSONObject("""{"playback":{"state":"paused","can_next":true}}""")).playback!!
        assertTrue(next.canNext)
        val legacy = Wire.parseUpdate(JSONObject("""{"playback":{"state":"playing"}}""")).playback!!
        assertTrue(legacy.canNext)
    }
}
