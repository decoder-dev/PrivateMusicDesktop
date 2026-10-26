package com.dec.privatemusic.mediasession

import com.dec.privatemusic.mediasession.FocusStateMachine.Change
import com.dec.privatemusic.mediasession.FocusStateMachine.Effect
import com.dec.privatemusic.mediasession.FocusStateMachine.RequestResult
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FocusStateMachineTest {
    private fun emit(e: ActionEvent) = Effect.Emit(e)

    private fun playing(): FocusStateMachine = FocusStateMachine().apply {
        assertEquals(listOf(Effect.RequestFocus), onPlaybackState(PlaybackKind.PLAYING))
        assertEquals(emptyList<Effect>(), onRequestResult(RequestResult.GRANTED))
    }

    @Test
    fun startingPlaybackRequestsFocusOnce() {
        val m = FocusStateMachine()
        assertEquals(listOf(Effect.RequestFocus), m.onPlaybackState(PlaybackKind.PLAYING))
        m.onRequestResult(RequestResult.GRANTED)
        assertEquals(emptyList<Effect>(), m.onPlaybackState(PlaybackKind.PLAYING))
        assertEquals(emptyList<Effect>(), m.onPlaybackState(PlaybackKind.BUFFERING))
        assertTrue(m.hasFocus)
    }

    @Test
    fun permanentLossPausesAndDoesNotComeBack() {
        val m = playing()
        assertEquals(listOf(emit(ActionEvent.PAUSE)), m.onFocusChange(Change.LOSS))
        assertFalse(m.hasFocus)
        assertFalse(m.pausedByFocus)
        m.onPlaybackState(PlaybackKind.PAUSED)
        // Another app that later gives focus back must not restart us.
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun transientLossPausesAndGainResumes() {
        val m = playing()
        assertEquals(listOf(emit(ActionEvent.PAUSE)), m.onFocusChange(Change.LOSS_TRANSIENT))
        assertTrue(m.pausedByFocus)
        // The host reports the pause we asked for: still "paused by focus".
        assertEquals(emptyList<Effect>(), m.onPlaybackState(PlaybackKind.PAUSED))
        assertTrue(m.pausedByFocus)
        assertEquals(listOf(emit(ActionEvent.PLAY)), m.onFocusChange(Change.GAIN))
        assertFalse(m.pausedByFocus)
        m.onPlaybackState(PlaybackKind.PLAYING)
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun pausedByTheUserIsNotResumedByAGain() {
        val m = playing()
        m.onPlaybackState(PlaybackKind.PAUSED) // the user pressed pause
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.LOSS_TRANSIENT))
        assertFalse(m.pausedByFocus)
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun resumingByHandCancelsTheAutomaticResume() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT)
        m.onPlaybackState(PlaybackKind.PAUSED)
        m.onPlaybackState(PlaybackKind.PLAYING) // the user pressed play during the call
        assertFalse(m.pausedByFocus)
        m.onPlaybackState(PlaybackKind.PAUSED) // and paused again
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun duckingIsReportedOnceAndUndoneOnGain() {
        val m = playing()
        assertEquals(listOf(emit(ActionEvent.duck(true))), m.onFocusChange(Change.LOSS_TRANSIENT_CAN_DUCK))
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.LOSS_TRANSIENT_CAN_DUCK))
        assertTrue(m.ducked)
        assertEquals(listOf(emit(ActionEvent.duck(false))), m.onFocusChange(Change.GAIN))
        assertFalse(m.ducked)
        // Never auto-pauses for a duck.
        assertFalse(m.pausedByFocus)
    }

    @Test
    fun noDuckWhenNothingPlays() {
        val m = FocusStateMachine()
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.LOSS_TRANSIENT_CAN_DUCK))
        assertFalse(m.ducked)
    }

    @Test
    fun aDuckThatTurnsIntoATransientLossEndsTheDuckThenPauses() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT_CAN_DUCK)
        assertEquals(
            listOf(emit(ActionEvent.duck(false)), emit(ActionEvent.PAUSE)),
            m.onFocusChange(Change.LOSS_TRANSIENT),
        )
        assertTrue(m.pausedByFocus)
    }

    @Test
    fun aDuckThatBecomesPermanentLossRestoresTheVolumeAndPauses() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT_CAN_DUCK)
        assertEquals(
            listOf(emit(ActionEvent.duck(false)), emit(ActionEvent.PAUSE)),
            m.onFocusChange(Change.LOSS),
        )
    }

    @Test
    fun aDelayedGrantPausesUntilTheFocusArrives() {
        val m = FocusStateMachine()
        m.onPlaybackState(PlaybackKind.PLAYING)
        assertEquals(listOf(emit(ActionEvent.PAUSE)), m.onRequestResult(RequestResult.DELAYED))
        assertTrue(m.pausedByFocus)
        m.onPlaybackState(PlaybackKind.PAUSED)
        assertEquals(listOf(emit(ActionEvent.PLAY)), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun aRefusedRequestPausesForGood() {
        val m = FocusStateMachine()
        m.onPlaybackState(PlaybackKind.PLAYING)
        assertEquals(listOf(emit(ActionEvent.PAUSE)), m.onRequestResult(RequestResult.FAILED))
        assertFalse(m.pausedByFocus)
        assertFalse(m.hasFocus)
    }

    @Test
    fun unpluggingHeadphonesPausesWithoutAutomaticResume() {
        val m = playing()
        assertEquals(listOf(emit(ActionEvent.PAUSE)), m.onBecomingNoisy())
        m.onPlaybackState(PlaybackKind.PAUSED)
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
        assertEquals(emptyList<Effect>(), m.onBecomingNoisy()) // nothing to pause now
    }

    @Test
    fun noisyAfterATransientLossCancelsTheResume() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT)
        m.onPlaybackState(PlaybackKind.PAUSED)
        m.onBecomingNoisy()
        assertFalse(m.pausedByFocus)
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun stoppingGivesTheFocusBackAndEndsADuck() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT_CAN_DUCK)
        assertEquals(
            listOf(emit(ActionEvent.duck(false)), Effect.AbandonFocus),
            m.onPlaybackState(PlaybackKind.STOPPED),
        )
        assertFalse(m.hasFocus)
    }

    @Test
    fun idlingKeepsWaitingForAGainButAnExplicitAbandonDoesNot() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT)
        m.onPlaybackState(PlaybackKind.PAUSED)
        assertEquals(emptyList<Effect>(), m.onIdle(force = false))
        assertTrue(m.pausedByFocus)
        assertEquals(listOf(Effect.AbandonFocus), m.onIdle(force = true))
        assertFalse(m.pausedByFocus)
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

    @Test
    fun idlingAfterAUserPauseAbandonsTheFocus() {
        val m = playing()
        m.onPlaybackState(PlaybackKind.PAUSED)
        assertEquals(listOf(Effect.AbandonFocus), m.onIdle(force = false))
        // Playing again asks for it afresh.
        assertEquals(listOf(Effect.RequestFocus), m.onPlaybackState(PlaybackKind.PLAYING))
    }
    @Test
    fun focusGainWaitsForTheAsynchronousPauseAcknowledgment() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT)
        // A position/capability snapshot from before Pause is still in flight.
        assertEquals(emptyList<Effect>(), m.onPlaybackState(PlaybackKind.PLAYING))
        assertTrue(m.pausedByFocus)
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
        assertEquals(listOf(emit(ActionEvent.PLAY)), m.onPlaybackState(PlaybackKind.PAUSED))
        assertEquals(emptyList<Effect>(), m.onPlaybackState(PlaybackKind.PAUSED))
    }

    @Test
    fun stopDuringTheFocusHandshakeCancelsAutomaticResume() {
        val m = playing()
        m.onFocusChange(Change.LOSS_TRANSIENT)
        m.onFocusChange(Change.GAIN)
        m.onPlaybackState(PlaybackKind.STOPPED)
        assertEquals(emptyList<Effect>(), m.onPlaybackState(PlaybackKind.PAUSED))
        assertEquals(emptyList<Effect>(), m.onFocusChange(Change.GAIN))
    }

}
