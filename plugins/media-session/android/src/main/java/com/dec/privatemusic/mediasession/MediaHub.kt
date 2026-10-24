package com.dec.privatemusic.mediasession

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject

/**
 * Process-wide state shared by the Tauri plugin (which dies with the Activity) and
 * [PlaybackService] (which must outlive it). Everything except [emit] and
 * [setChannel] runs on the main thread: the plugin posts here.
 */
object MediaHub {
    private val main = Handler(Looper.getMainLooper())

    @Volatile
    private var channel: Channel? = null
    private var context: Context? = null
    private var focus: AudioFocusController? = null
    private var preferences: PlaybackPreferences? = null
    private val artworkLoader = ArtworkLoader()

    var track: TrackInfo? = null
        private set
    var snapshot: PlaybackSnapshot? = null
        private set
    var artwork: Bitmap? = null
        private set
    private var artworkUrl: String? = null
    private var artworkOverride: String? = null

    fun attach(applicationContext: Context) {
        if (context == null) context = applicationContext
        if (preferences == null) preferences = PlaybackPreferences(applicationContext)
        if (focus == null) {
            main.post { if (focus == null) focus = AudioFocusController(applicationContext, ::emit) }
        }
    }

    fun setChannel(newChannel: Channel) {
        channel = newChannel
    }

    /** Sends a request from the system to the app. Safe from any thread. */
    fun emit(event: ActionEvent) {
        val target = channel ?: return
        try {
            target.send(JSObject(Wire.toJson(event).toString()))
        } catch (e: Exception) {
            Log.w(TAG, "could not deliver ${event.name}: ${e.javaClass.simpleName}")
        }
    }

    fun onUpdate(update: Wire.Update) {
        if (update.clearTrack) {
            track = null
            setArtwork(null, null)
        }
        update.track?.let {
            track = it
            reloadArtwork()
        }
        update.playback?.let {
            snapshot = PlaybackSnapshot(it, SystemClock.elapsedRealtime())
            focus?.onPlaybackState(it.kind)
        }
        preferences?.onPlaybackChanged()
        syncService()
    }

    fun configurePlayback(resumeBluetooth: Boolean, pauseAtZero: Boolean) {
        preferences?.configure(resumeBluetooth, pauseAtZero)
    }

    fun setArtworkOverride(url: String?) {
        artworkOverride = url
        reloadArtwork()
        PlaybackService.instance?.refresh()
    }

    fun requestFocus() {
        focus?.requestNow()
    }

    fun abandonFocus() {
        focus?.abandon()
    }

    fun onServiceIdle() {
        focus?.onIdle()
    }

    fun onBecomingNoisy() {
        focus?.onBecomingNoisy()
    }

    /** The `stop` command: forget everything and end the service. */
    fun stop() {
        track = null
        snapshot = null
        setArtwork(null, null)
        focus?.abandon()
        PlaybackService.instance?.shutdown()
    }

    /** Called by the service when it has shut down. */
    fun onServiceEnded() {
        focus?.abandon()
    }

    private fun reloadArtwork() {
        val url = artworkOverride ?: track?.artworkUrl
        if (!ArtworkPolicy.isAllowedUrl(url)) {
            setArtwork(null, null)
            return
        }
        if (url == artworkUrl) return // already loaded or loading
        setArtwork(null, url)
        artworkLoader.load(url!!) { bitmap ->
            main.post {
                if (artworkUrl == url) {
                    artwork = bitmap
                    PlaybackService.instance?.refresh()
                }
            }
        }
    }

    private fun setArtwork(bitmap: Bitmap?, url: String?) {
        if (url == null) artworkLoader.cancel()
        artwork = bitmap
        artworkUrl = url
    }

    private fun syncService() {
        val running = PlaybackService.instance
        if (running != null) {
            running.refresh()
            return
        }
        if (snapshot?.info?.kind?.isActive != true) return
        val ctx = context ?: return
        try {
            ContextCompat.startForegroundService(
                ctx,
                Intent(ctx, PlaybackService::class.java).setAction(PlaybackService.ACTION_START),
            )
        } catch (e: RuntimeException) {
            // Typically ForegroundServiceStartNotAllowedException from the background.
            // Playback itself is unaffected; only the notification is missing.
            Log.w(TAG, "playback service could not start: ${e.javaClass.simpleName}")
        }
    }

    private const val TAG = "MediaSession"
}
