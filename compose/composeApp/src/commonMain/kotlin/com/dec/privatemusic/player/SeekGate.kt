package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.time.TimeMark
import kotlin.time.TimeSource

/** Holds the requested position until the engine confirms it, then releases the slider. */
class SeekGate(
    private val scope: CoroutineScope,
    private val timeoutMs: Long = 1200,
    private val onSettle: () -> Unit = {},
) {
    private var generation = 0L
    private var trackKey: String? = null
    private var pendingValue: Double? = null
    private var pendingSince: TimeMark? = null
    private var timer: Job? = null
    private var dragging = false

    fun preview() {
        generation += 1
        timer?.cancel()
        pendingValue = null
        pendingSince = null
        dragging = true
    }

    fun track(key: String?) {
        if (key == trackKey) return
        trackKey = key
        cancel()
    }

    fun cancel() {
        generation += 1
        timer?.cancel()
        pendingValue = null
        pendingSince = null
        dragging = false
    }

    fun commit(value: Double) {
        val mine = ++generation
        timer?.cancel()
        pendingValue = value
        pendingSince = TimeSource.Monotonic.markNow()
        dragging = true
        timer = scope.launch {
            delay(timeoutMs)
            if (mine == generation) {
                dragging = false
                pendingValue = null
                pendingSince = null
                onSettle()
            }
        }
    }

    fun shouldRender(position: Double): Boolean {
        val requested = pendingValue ?: return !dragging
        if (kotlin.math.abs(position.coerceAtLeast(0.0) - requested) <= 1.1) {
            cancel()
            return true
        }
        val since = pendingSince
        if (since != null && since.elapsedNow().inWholeMilliseconds >= timeoutMs) {
            cancel()
            return true
        }
        return false
    }
}
