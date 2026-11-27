package com.dec.privatemusic.player

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class SeekGateTest {
    @Test
    fun oldPacketCannotReplaceRequestedPosition() = runTest {
        val gate = SeekGate(this)
        gate.preview()
        gate.commit(90.0)
        assertFalse(gate.shouldRender(10.0))
        assertTrue(gate.shouldRender(90.0))
        assertTrue(gate.shouldRender(90.5))
    }

    @Test
    fun acknowledgementAloneDoesNotReleaseSlider() = runTest {
        val gate = SeekGate(this)
        gate.commit(75.0)
        assertFalse(gate.shouldRender(4.0))
    }

    @Test
    fun timeoutReturnsToRealPosition() = runTest {
        val gate = SeekGate(this, timeoutMs = 100)
        gate.commit(80.0)
        assertFalse(gate.shouldRender(8.0))
        advanceTimeBy(150)
        assertTrue(gate.shouldRender(8.0))
    }

    @Test
    fun newerGestureWinsOverOlderRequest() = runTest {
        val gate = SeekGate(this)
        gate.commit(30.0)
        gate.preview()
        assertFalse(gate.shouldRender(12.0))
    }

    @Test
    fun trackChangeCancelsPendingPosition() = runTest {
        val gate = SeekGate(this)
        gate.commit(80.0)
        gate.track("7_2")
        assertTrue(gate.shouldRender(0.0))
    }
}
