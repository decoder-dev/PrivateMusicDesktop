package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SmokeModeTest {
    @Test
    fun onlyDebugBuildsHonourTheExtra() {
        assertTrue(SmokeMode.requested(true, null, true))
        assertTrue(SmokeMode.requested(false, "1", true))
        assertTrue(SmokeMode.requested(false, "true", true))
        assertFalse(SmokeMode.requested(false, null, true))
        assertFalse(SmokeMode.requested(false, "0", true))
        assertFalse("release builds ignore it", SmokeMode.requested(true, "1", false))
        assertEquals("loaded vk.ru", SmokeMode.loadedLine("vk.ru"))
    }
}
