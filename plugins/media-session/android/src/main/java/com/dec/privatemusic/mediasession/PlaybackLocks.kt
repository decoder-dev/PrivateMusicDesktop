package com.dec.privatemusic.mediasession

import android.content.Context
import android.net.wifi.WifiManager
import android.os.PowerManager
import android.util.Log

/** CPU and Wi-Fi locks held only while sound is wanted. Main thread only. */
class PlaybackLocks(context: Context) {
    private val wake: PowerManager.WakeLock? =
        (context.getSystemService(Context.POWER_SERVICE) as? PowerManager)
            ?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "PrivateMusic:playback")
            ?.apply { setReferenceCounted(false) }

    @Suppress("DEPRECATION") // FULL_HIGH_PERF is the right mode up to API 28 and is mapped to low-latency above.
    private val wifi: WifiManager.WifiLock? =
        (context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager)
            ?.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "PrivateMusic:stream")
            ?.apply { setReferenceCounted(false) }

    /** Call on every playback message: re-acquiring with a timeout renews the safety net. */
    fun update(kind: PlaybackKind) {
        try {
            if (LockPolicy.needsWakeLock(kind)) wake?.acquire(LockPolicy.WAKE_LOCK_TIMEOUT_MS) else release(wake)
            if (LockPolicy.needsWifiLock(kind)) {
                if (wifi?.isHeld == false) wifi.acquire()
            } else {
                release(wifi)
            }
        } catch (e: SecurityException) {
            Log.w(TAG, "wake lock permission missing")
        }
    }

    fun releaseAll() {
        release(wake)
        release(wifi)
    }

    private fun release(lock: PowerManager.WakeLock?) {
        if (lock?.isHeld == true) lock.release()
    }

    private fun release(lock: WifiManager.WifiLock?) {
        if (lock?.isHeld == true) lock.release()
    }

    private companion object {
        const val TAG = "MediaSession"
    }
}
