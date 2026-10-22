package com.dec.privatemusic.mediasession

import com.dec.privatemusic.mediasession.ForegroundPolicy.Decision
import com.dec.privatemusic.mediasession.ForegroundPolicy.Mode
import org.junit.Assert.assertEquals
import org.junit.Test

class ForegroundPolicyTest {
    private val config = ForegroundPolicy.Config(pauseGraceMs = 30_000, idleStopMs = 600_000, stoppedGraceMs = 5_000)

    @Test
    fun playingAndBufferingStayForegroundWithoutTimer() {
        val p = ForegroundPolicy(config)
        assertEquals(Decision(Mode.FOREGROUND, null), p.onPlayback(PlaybackKind.PLAYING, 1_000))
        assertEquals(Decision(Mode.FOREGROUND, null), p.onPlayback(PlaybackKind.BUFFERING, 50_000))
        assertEquals(Decision(Mode.FOREGROUND, null), p.decide(10_000_000))
    }

    @Test
    fun pauseKeepsForegroundForTheGraceThenDetachesThenStops() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        assertEquals(Decision(Mode.FOREGROUND, 40_000), p.onPlayback(PlaybackKind.PAUSED, 10_000))
        assertEquals(Decision(Mode.FOREGROUND, 40_000), p.decide(39_999))
        assertEquals(Decision(Mode.DETACHED, 610_000), p.decide(40_000))
        assertEquals(Decision(Mode.DETACHED, 610_000), p.decide(609_999))
        assertEquals(Decision(Mode.STOPPED, null), p.decide(610_000))
    }

    @Test
    fun repeatedPausedMessagesDoNotRestartTheClock() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PAUSED, 0)
        assertEquals(Decision(Mode.DETACHED, 600_000), p.onPlayback(PlaybackKind.PAUSED, 31_000))
    }

    @Test
    fun resumingFromDetachedGoesBackToForeground() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PAUSED, 0)
        assertEquals(Mode.DETACHED, p.decide(100_000).mode)
        assertEquals(Decision(Mode.FOREGROUND, null), p.onPlayback(PlaybackKind.PLAYING, 100_000))
    }

    @Test
    fun swipingTheNotificationWhilePausedStopsAtOnce() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PAUSED, 0)
        p.decide(50_000)
        assertEquals(Decision(Mode.STOPPED, null), p.onNotificationDismissed(60_000))
    }

    @Test
    fun aDismissalWhilePlayingIsIgnored() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        assertEquals(Decision(Mode.FOREGROUND, null), p.onNotificationDismissed(1_000))
        // ...and does not poison a later pause.
        assertEquals(Mode.FOREGROUND, p.onPlayback(PlaybackKind.PAUSED, 2_000).mode)
    }

    @Test
    fun stoppedLeavesAfterTheShortGrace() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        assertEquals(Decision(Mode.FOREGROUND, 15_000), p.onPlayback(PlaybackKind.STOPPED, 10_000))
        assertEquals(Decision(Mode.STOPPED, null), p.decide(15_000))
    }

    @Test
    fun aTrackChangeThroughStoppedDoesNotKillTheService() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        p.onPlayback(PlaybackKind.STOPPED, 10_000)
        assertEquals(Decision(Mode.FOREGROUND, null), p.onPlayback(PlaybackKind.PLAYING, 11_000))
    }

    @Test
    fun aServiceStartedWithoutStateLeavesOnItsOwn() {
        val p = ForegroundPolicy(config, startedAtMs = 100)
        assertEquals(Decision(Mode.FOREGROUND, 5_100), p.decide(200))
        assertEquals(Mode.STOPPED, p.decide(5_100).mode)
    }

    @Test
    fun removingTheTaskWhilePlayingKeepsPlaying() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        assertEquals(Decision(Mode.FOREGROUND, null), p.onTaskRemoved(5_000))
        p.onPlayback(PlaybackKind.BUFFERING, 6_000)
        assertEquals(Mode.FOREGROUND, p.onTaskRemoved(7_000).mode)
    }

    @Test
    fun removingTheTaskWhilePausedOrStoppedLeavesForGood() {
        val paused = ForegroundPolicy(config)
        paused.onPlayback(PlaybackKind.PAUSED, 0)
        assertEquals(Decision(Mode.STOPPED, null), paused.onTaskRemoved(1_000))
        // Even if the app is somehow reported as playing afterwards, the service is done.
        assertEquals(Mode.STOPPED, paused.onPlayback(PlaybackKind.PLAYING, 2_000).mode)

        val stopped = ForegroundPolicy(config)
        assertEquals(Mode.STOPPED, stopped.onTaskRemoved(1_000).mode)
    }

    @Test
    fun aForegroundServiceTimeoutStopsCleanly() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        assertEquals(Decision(Mode.STOPPED, null), p.onTimeout())
        assertEquals(Decision(Mode.STOPPED, null), p.decide(1))
    }

    @Test
    fun endStopsImmediately() {
        val p = ForegroundPolicy(config)
        p.onPlayback(PlaybackKind.PLAYING, 0)
        assertEquals(Decision(Mode.STOPPED, null), p.end())
    }
}
