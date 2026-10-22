package com.dec.privatemusic.mediasession

/** Android 13+ notification permission rules. Denial must never stop playback. */
object PermissionPolicy {
    const val TIRAMISU = 33

    /**
     * Ask when playback starts, once: a second system dialog right after a denial
     * would be annoying (and Android stops showing it after two denials anyway).
     */
    fun shouldAskForNotifications(sdkInt: Int, granted: Boolean, alreadyAsked: Boolean): Boolean =
        sdkInt >= TIRAMISU && !granted && !alreadyAsked

    /** Below Android 13 notifications need no permission. */
    fun notificationsEnabled(sdkInt: Int, granted: Boolean, enabledInSettings: Boolean): Boolean =
        enabledInSettings && (sdkInt < TIRAMISU || granted)
}

/** Which locks the playing state needs. */
object LockPolicy {
    /** Safety net: a lock never outlives a day, whatever happens to the service. */
    const val WAKE_LOCK_TIMEOUT_MS = 6L * 60 * 60 * 1000

    fun needsWakeLock(kind: PlaybackKind): Boolean = kind.isActive

    /** The tracks are streamed from the network; buffering also needs the radio awake. */
    fun needsWifiLock(kind: PlaybackKind): Boolean = kind.isActive
}
