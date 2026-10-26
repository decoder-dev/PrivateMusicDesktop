package com.dec.privatemusic.mediasession

import android.content.Context
import android.database.ContentObserver
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.provider.Settings

/** Process-owned observers; they survive Activity recreation with MediaHub. */
class PlaybackPreferences(private val context: Context) {
    private val main = Handler(Looper.getMainLooper())
    private val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val known = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).map { it.id }.toMutableSet()
    private var resumeBluetooth = true
    private var pauseAtZero = false
    // Pause only when the volume is lowered to zero. A deliberate Play at zero stays playing.
    private var lastVolume = audio.getStreamVolume(AudioManager.STREAM_MUSIC)
    private val devices = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) {
            var connected = false
            addedDevices.forEach { device ->
                val fresh = known.add(device.id)
                if (fresh && device.isSink && device.type in BLUETOOTH_OUTPUTS) connected = true
            }
            // Registering the callback also delivers devices that were already connected.
            if (connected && resumeBluetooth && audio.mode == AudioManager.MODE_NORMAL && MediaHub.track != null &&
                MediaHub.snapshot?.info?.kind == PlaybackKind.PAUSED &&
                (!pauseAtZero || audio.getStreamVolume(AudioManager.STREAM_MUSIC) > 0)) {
                MediaHub.emit(ActionEvent.PLAY)
            }
        }
        override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) {
            removedDevices.forEach { known.remove(it.id) }
        }
    }
    private val volume = object : ContentObserver(main) {
        override fun onChange(selfChange: Boolean) { onVolumeChanged() }
    }
    init {
        audio.registerAudioDeviceCallback(devices, main)
        context.contentResolver.registerContentObserver(Settings.System.CONTENT_URI, true, volume)
    }
    fun configure(resume: Boolean, pause: Boolean) {
        resumeBluetooth = resume
        pauseAtZero = pause
        lastVolume = audio.getStreamVolume(AudioManager.STREAM_MUSIC)
    }
    private fun onVolumeChanged() {
        val current = audio.getStreamVolume(AudioManager.STREAM_MUSIC)
        val lowered = lastVolume > 0 && current == 0
        lastVolume = current
        if (pauseAtZero && lowered && MediaHub.snapshot?.info?.kind?.isActive == true) {
            MediaHub.emit(ActionEvent.PAUSE.copy(source = "zero_volume"))
        }
    }
    companion object {
        private val BLUETOOTH_OUTPUTS = setOf(
            AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_BLE_HEADSET,
            AudioDeviceInfo.TYPE_BLE_SPEAKER, AudioDeviceInfo.TYPE_HEARING_AID,
        )
    }
}
