package com.dec.privatemusic.mediasession

import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import android.util.Log
import android.view.KeyEvent
import androidx.core.app.ServiceCompat
import androidx.core.content.IntentCompat

/**
 * Foreground service (`mediaPlayback`) that keeps the process alive while the Rust
 * engine plays, and owns the `MediaSession`: notification, lock screen, Bluetooth
 * and headset buttons. The audio itself is not here; this class mirrors the state
 * the host reports through [MediaHub] and forwards button presses back.
 *
 * Main thread only.
 */
class PlaybackService : Service() {
    private lateinit var session: MediaSessionCompat
    private lateinit var notifications: NotificationFactory
    private lateinit var locks: PlaybackLocks
    private lateinit var policy: ForegroundPolicy
    private val handler = Handler(Looper.getMainLooper())
    private val recheck = Runnable { apply(policy.decide(SystemClock.elapsedRealtime())) }

    private var foreground = false
    private var mustEnterForeground = false
    private var noisyRegistered = false
    private var ended = false
    private var shownTrack: TrackInfo? = null
    private var shownArtwork: android.graphics.Bitmap? = null

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) MediaHub.onBecomingNoisy()
        }
    }

    override fun onCreate() {
        super.onCreate()
        instance = this
        notifications = NotificationFactory(this)
        locks = PlaybackLocks(this)
        policy = ForegroundPolicy(startedAtMs = SystemClock.elapsedRealtime())
        session = MediaSessionCompat(this, "PrivateMusic").apply {
            setCallback(SessionCallback())
            packageManager.getLaunchIntentForPackage(packageName)?.let {
                setSessionActivity(
                    android.app.PendingIntent.getActivity(
                        this@PlaybackService, 0, it, android.app.PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            }
            isActive = true
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        val button = ButtonId.fromIntentAction(action)
        when {
            action == ACTION_START -> {
                // startForegroundService() promises a startForeground() within seconds,
                // whatever the state is by now.
                mustEnterForeground = true
                if (!foreground) enterForeground() // the promise holds even if we are already finishing
                refresh()
            }
            button != null -> MediaHub.emit(button.toEvent())
            action == ACTION_DISMISSED -> apply(policy.onNotificationDismissed(SystemClock.elapsedRealtime()))
            else -> refresh()
        }
        // Without the app there is nothing to restore: never ask for a restart.
        return START_NOT_STICKY
    }

    /** `Service.onTimeout` for API 35 (foreground service type timeouts). mediaPlayback has none today. */
    override fun onTimeout(startId: Int, fgsType: Int) = handleTimeout()

    /** `Service.onTimeout` for API 34. */
    override fun onTimeout(startId: Int) = handleTimeout()

    private fun handleTimeout() {
        // Must stop within seconds or the system crashes the app: tell the host first so
        // the sound stops too, then leave.
        MediaHub.emit(ActionEvent.STOP)
        apply(policy.onTimeout())
    }

    /** The app was swiped from recents: keep playing, otherwise leave. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        apply(policy.onTaskRemoved(SystemClock.elapsedRealtime()))
    }

    override fun onDestroy() {
        if (instance === this) instance = null
        handler.removeCallbacks(recheck)
        setNoisyReceiver(false)
        locks.releaseAll()
        session.isActive = false
        session.release()
        if (!ended) MediaHub.onServiceEnded()
        super.onDestroy()
    }

    /** The `stop` command and the end of the policy. */
    fun shutdown() = apply(policy.end())

    /** Re-reads [MediaHub] and refreshes session, notification, locks and policy. */
    fun refresh() {
        if (ended) return
        val snapshot = MediaHub.snapshot
        val kind = snapshot?.info?.kind ?: PlaybackKind.STOPPED
        val track = MediaHub.track
        pushSession(track, snapshot, kind)
        locks.update(kind)
        setNoisyReceiver(kind.isActive)
        apply(policy.onPlayback(kind, SystemClock.elapsedRealtime()))
    }

    private fun pushSession(track: TrackInfo?, snapshot: PlaybackSnapshot?, kind: PlaybackKind) {
        val artwork = MediaHub.artwork
        if (track != shownTrack || artwork !== shownArtwork) {
            shownTrack = track
            shownArtwork = artwork
            session.setMetadata(buildMetadata(track, artwork))
        }
        val info = snapshot?.info ?: PlaybackInfo(PlaybackKind.STOPPED, 0, 0, 1f)
        val now = SystemClock.elapsedRealtime()
        val position = snapshot?.positionAt(now) ?: 0
        val mapped = PlaybackStateMapper.map(info, track != null, position)
        session.setPlaybackState(
            PlaybackStateCompat.Builder()
                .setActions(mapped.actions)
                .setState(mapped.state, mapped.positionMs, mapped.speed, now)
                .build(),
        )
        if (kind == PlaybackKind.STOPPED && track == null) shownTrack = null
    }

    private fun buildMetadata(track: TrackInfo?, artwork: android.graphics.Bitmap?): MediaMetadataCompat {
        val builder = MediaMetadataCompat.Builder()
        if (track != null) {
            builder.putString(MediaMetadataCompat.METADATA_KEY_TITLE, track.title)
                .putString(MediaMetadataCompat.METADATA_KEY_DISPLAY_TITLE, track.title)
                .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, track.artist)
                .putString(MediaMetadataCompat.METADATA_KEY_DISPLAY_SUBTITLE, track.artist)
            track.album?.let { builder.putString(MediaMetadataCompat.METADATA_KEY_ALBUM, it) }
            if (track.durationMs > 0) builder.putLong(MediaMetadataCompat.METADATA_KEY_DURATION, track.durationMs)
        }
        if (artwork != null) {
            builder.putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, artwork)
                .putBitmap(MediaMetadataCompat.METADATA_KEY_DISPLAY_ICON, artwork)
        }
        return builder.build()
    }

    private fun currentNotification(): Notification {
        val kind = MediaHub.snapshot?.info?.kind ?: PlaybackKind.STOPPED
        return notifications.build(session.sessionToken, MediaHub.track, kind, MediaHub.artwork, foreground)
    }

    private fun apply(decision: ForegroundPolicy.Decision) {
        handler.removeCallbacks(recheck)
        if (ended) return
        when (decision.mode) {
            ForegroundPolicy.Mode.STOPPED -> {
                finish()
                return
            }
            ForegroundPolicy.Mode.FOREGROUND -> {
                val kind = MediaHub.snapshot?.info?.kind ?: PlaybackKind.STOPPED
                if (!foreground && (mustEnterForeground || kind.isActive)) {
                    enterForeground()
                } else {
                    post()
                }
            }
            ForegroundPolicy.Mode.DETACHED -> {
                // Nothing is playing any more: let the process go, keep a swipeable notification.
                if (mustEnterForeground && !foreground) enterForeground()
                if (foreground) {
                    ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_DETACH)
                    foreground = false
                }
                post()
                MediaHub.onServiceIdle()
            }
        }
        decision.recheckAtMs?.let { at ->
            handler.postDelayed(recheck, (at - SystemClock.elapsedRealtime()).coerceAtLeast(0))
        }
    }

    private fun enterForeground() {
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
        } else {
            0
        }
        try {
            ServiceCompat.startForeground(this, NOTIFICATION_ID, currentNotification(), type)
            foreground = true
            mustEnterForeground = false
        } catch (e: RuntimeException) {
            // ForegroundServiceStartNotAllowedException (API 31+, resumed from the background) or a
            // missing permission: the engine keeps playing, we just cannot promise to stay alive.
            Log.w(TAG, "could not enter foreground: ${e.javaClass.simpleName}")
            foreground = false
            mustEnterForeground = false
            post()
        }
    }

    private fun post() {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        try {
            manager.notify(NOTIFICATION_ID, currentNotification())
        } catch (e: RuntimeException) {
            Log.w(TAG, "could not post the notification: ${e.javaClass.simpleName}")
        }
    }

    private fun finish() {
        ended = true
        // Later updates must start a fresh service, not feed this dying one.
        if (instance === this) instance = null
        handler.removeCallbacks(recheck)
        setNoisyReceiver(false)
        locks.releaseAll()
        MediaHub.onServiceEnded()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        (getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(NOTIFICATION_ID)
        foreground = false
        stopSelf()
    }

    private fun setNoisyReceiver(on: Boolean) {
        if (on == noisyRegistered) return
        noisyRegistered = on
        if (on) {
            registerReceiver(noisyReceiver, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY))
        } else {
            try {
                unregisterReceiver(noisyReceiver)
            } catch (_: IllegalArgumentException) {
            }
        }
    }

    private inner class SessionCallback : MediaSessionCompat.Callback() {
        override fun onPlay() = MediaHub.emit(ActionEvent.PLAY)
        override fun onPause() = MediaHub.emit(ActionEvent.PAUSE)
        override fun onStop() = MediaHub.emit(ActionEvent.STOP)
        override fun onSkipToNext() = MediaHub.emit(ActionEvent.NEXT)
        override fun onSkipToPrevious() = MediaHub.emit(ActionEvent.PREVIOUS)
        override fun onSeekTo(pos: Long) = MediaHub.emit(ActionEvent.seekTo(pos.coerceAtLeast(0) / 1000.0))
        override fun onFastForward() = MediaHub.emit(ActionEvent.seekBy(MediaKeyMapper.SEEK_STEP_SECONDS))
        override fun onRewind() = MediaHub.emit(ActionEvent.seekBy(-MediaKeyMapper.SEEK_STEP_SECONDS))

        override fun onMediaButtonEvent(mediaButtonEvent: Intent): Boolean {
            val event = IntentCompat.getParcelableExtra(mediaButtonEvent, Intent.EXTRA_KEY_EVENT, KeyEvent::class.java)
            val action = event?.let { MediaKeyMapper.actionFor(it.keyCode) }
            if (event != null && action != null) {
                // Handled here whatever actions the PlaybackState advertises, with the screen off or not.
                if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount == 0) MediaHub.emit(action)
                return true
            }
            // KEYCODE_HEADSETHOOK: single click play/pause, double next, triple previous.
            return super.onMediaButtonEvent(mediaButtonEvent)
        }
    }

    companion object {
        const val ACTION_START = "com.dec.privatemusic.mediasession.START"
        const val ACTION_DISMISSED = "com.dec.privatemusic.mediasession.DISMISSED"
        private const val NOTIFICATION_ID = 0x504D // "PM"
        private const val TAG = "MediaSession"

        @Volatile
        var instance: PlaybackService? = null
            private set
    }
}
