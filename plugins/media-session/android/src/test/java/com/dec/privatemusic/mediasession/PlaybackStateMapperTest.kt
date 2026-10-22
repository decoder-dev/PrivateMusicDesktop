package com.dec.privatemusic.mediasession

import android.support.v4.media.session.PlaybackStateCompat as P
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackStateMapperTest {
    private fun info(kind: PlaybackKind, position: Long = 5_000, duration: Long = 200_000, rate: Float = 1f) =
        PlaybackInfo(kind, position, duration, rate)

    private fun has(actions: Long, flag: Long) = actions and flag != 0L

    @Test
    fun playingKeepsTheRateSoTheSystemAdvancesTheScrubber() {
        val m = PlaybackStateMapper.map(info(PlaybackKind.PLAYING, rate = 1f), true)
        assertEquals(P.STATE_PLAYING, m.state)
        assertEquals(1f, m.speed, 0f)
        assertEquals(5_000, m.positionMs)
    }

    @Test
    fun pausedAndBufferingStandStill() {
        val paused = PlaybackStateMapper.map(info(PlaybackKind.PAUSED), true)
        assertEquals(P.STATE_PAUSED, paused.state)
        assertEquals(0f, paused.speed, 0f)
        val buffering = PlaybackStateMapper.map(info(PlaybackKind.BUFFERING), true)
        assertEquals(P.STATE_BUFFERING, buffering.state)
        assertEquals(0f, buffering.speed, 0f)
        assertEquals(5_000, buffering.positionMs)
    }

    @Test
    fun stoppedResetsThePositionAndOffersOnlyPlay() {
        val withTrack = PlaybackStateMapper.map(info(PlaybackKind.STOPPED, position = 9_000), true)
        assertEquals(P.STATE_STOPPED, withTrack.state)
        assertEquals(0, withTrack.positionMs)
        assertTrue(has(withTrack.actions, P.ACTION_PLAY))
        assertEquals(0L, withTrack.actions and P.ACTION_SKIP_TO_NEXT)
        assertEquals(0L, PlaybackStateMapper.map(info(PlaybackKind.STOPPED), false).actions)
    }

    @Test
    fun activeStatesAdvertiseEveryTransportAction() {
        for (kind in listOf(PlaybackKind.PLAYING, PlaybackKind.PAUSED, PlaybackKind.BUFFERING)) {
            val a = PlaybackStateMapper.map(info(kind), true).actions
            for (flag in listOf(
                P.ACTION_PLAY, P.ACTION_PAUSE, P.ACTION_PLAY_PAUSE, P.ACTION_SKIP_TO_NEXT,
                P.ACTION_SKIP_TO_PREVIOUS, P.ACTION_STOP, P.ACTION_SEEK_TO,
            )) {
                assertTrue("$kind lacks $flag", has(a, flag))
            }
        }
    }

    @Test
    fun seekingIsOfferedOnlyWhenTheDurationIsKnown() {
        val unknown = PlaybackStateMapper.map(info(PlaybackKind.PLAYING, duration = 0), true)
        assertEquals(0L, unknown.actions and P.ACTION_SEEK_TO)
        assertTrue(has(unknown.actions, P.ACTION_SKIP_TO_NEXT))
    }

    @Test
    fun positionIsClampedToTheTrack() {
        assertEquals(200_000, PlaybackStateMapper.map(info(PlaybackKind.PAUSED, position = 999_999), true).positionMs)
        assertEquals(0, PlaybackStateMapper.map(info(PlaybackKind.PAUSED, position = -5), true).positionMs)
        assertEquals(1_234_567, PlaybackStateMapper.map(info(PlaybackKind.PLAYING, position = 1_234_567, duration = 0), true).positionMs)
    }

    @Test
    fun anExplicitPositionOverridesTheReportedOne() {
        assertEquals(42_000, PlaybackStateMapper.map(info(PlaybackKind.PLAYING), true, positionMs = 42_000).positionMs)
    }

    @Test
    fun extrapolationAdvancesOnlyWhilePlaying() {
        val playing = info(PlaybackKind.PLAYING, position = 10_000, rate = 1f)
        assertEquals(10_000, PlaybackStateMapper.extrapolate(playing, 1_000, 1_000))
        assertEquals(13_000, PlaybackStateMapper.extrapolate(playing, 1_000, 4_000))
        assertEquals(10_000, PlaybackStateMapper.extrapolate(playing, 1_000, 500)) // clock never runs backwards
        assertEquals(12_000, PlaybackStateMapper.extrapolate(info(PlaybackKind.PLAYING, position = 10_000, rate = 2f), 0, 1_000))
        assertEquals(10_000, PlaybackStateMapper.extrapolate(info(PlaybackKind.PAUSED, position = 10_000), 0, 60_000))
        assertEquals(10_000, PlaybackStateMapper.extrapolate(info(PlaybackKind.BUFFERING, position = 10_000), 0, 60_000))
        assertEquals(200_000, PlaybackStateMapper.extrapolate(playing, 0, 10_000_000))
    }

    @Test
    fun aSnapshotKnowsWhereThePlayheadIsNow() {
        val snapshot = PlaybackSnapshot(info(PlaybackKind.PLAYING, position = 1_000), receivedAtMs = 100)
        assertEquals(3_000, snapshot.positionAt(2_100))
    }
}
