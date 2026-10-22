package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Test

class LoadWatchTest {
    private fun s(n: Long) = n * 1000

    @Test
    fun aLoginPageThatDoesNotArriveIsReported() {
        assertEquals(LoadVerdict.WAITING, LoadWatch.verdict(s(3), false, false))
        assertEquals(LoadVerdict.WAITING, LoadWatch.verdict(24_999, false, false))
        assertEquals(LoadVerdict.FAILED, LoadWatch.verdict(s(25), false, false))
        assertEquals(LoadVerdict.WAITING, LoadWatch.verdict(s(2), false, true))
        assertEquals(LoadVerdict.FAILED, LoadWatch.verdict(s(4), false, true))
        assertEquals(LoadVerdict.WAITING, LoadWatch.verdict(s(600), true, false))
        assertEquals(LoadVerdict.WAITING, LoadWatch.verdict(s(600), true, true))
    }
}
