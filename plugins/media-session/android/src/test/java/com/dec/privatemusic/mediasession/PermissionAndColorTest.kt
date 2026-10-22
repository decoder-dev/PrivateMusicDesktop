package com.dec.privatemusic.mediasession

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PermissionAndColorTest {
    @Test
    fun notificationPermissionIsAskedOnceOnAndroid13Plus() {
        assertTrue(PermissionPolicy.shouldAskForNotifications(33, granted = false, alreadyAsked = false))
        assertTrue(PermissionPolicy.shouldAskForNotifications(35, granted = false, alreadyAsked = false))
        assertFalse(PermissionPolicy.shouldAskForNotifications(33, granted = false, alreadyAsked = true))
        assertFalse(PermissionPolicy.shouldAskForNotifications(34, granted = true, alreadyAsked = false))
        assertFalse(PermissionPolicy.shouldAskForNotifications(32, granted = false, alreadyAsked = false))
        assertFalse(PermissionPolicy.shouldAskForNotifications(26, granted = false, alreadyAsked = false))
    }

    @Test
    fun deniedPermissionOnlyHidesTheNotification() {
        assertTrue(PermissionPolicy.notificationsEnabled(30, granted = false, enabledInSettings = true))
        assertFalse(PermissionPolicy.notificationsEnabled(34, granted = false, enabledInSettings = true))
        assertFalse(PermissionPolicy.notificationsEnabled(34, granted = true, enabledInSettings = false))
        assertTrue(PermissionPolicy.notificationsEnabled(34, granted = true, enabledInSettings = true))
    }

    @Test
    fun locksAreHeldOnlyWhileSoundIsWanted() {
        for (kind in PlaybackKind.entries) {
            assertEquals(kind.isActive, LockPolicy.needsWakeLock(kind))
            assertEquals(kind.isActive, LockPolicy.needsWifiLock(kind))
        }
        assertTrue(LockPolicy.WAKE_LOCK_TIMEOUT_MS in 1..(24L * 3600 * 1000))
    }

    @Test
    fun brightArtworkIsDarkenedForWhiteText() {
        val white = NotificationColor.forBackground(0xFFFFFFFF.toInt())
        assertTrue(NotificationColor.luminance((white shr 16) and 0xFF, (white shr 8) and 0xFF, white and 0xFF) <= 0.30)
        assertEquals(0xFF, (white ushr 24))
    }

    @Test
    fun darkArtworkIsKeptAndMadeOpaque() {
        assertEquals(0xFF102030.toInt(), NotificationColor.forBackground(0x00102030))
    }
}
