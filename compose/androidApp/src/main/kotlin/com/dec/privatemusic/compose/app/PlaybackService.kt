package com.dec.privatemusic.compose.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Build
import android.os.IBinder
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import com.dec.privatemusic.player.PlayerApi
import com.dec.privatemusic.player.PlayerState
import com.dec.privatemusic.player.Track
import com.dec.privatemusic.player.isPlayerStateEvent
import com.dec.privatemusic.player.platformPlayerApi
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject

/** Native Android transport controls backed by the same Rust engine as the Compose UI. */
class PlaybackService : android.app.Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val json = Json { ignoreUnknownKeys = true }
    private lateinit var api: PlayerApi
    private lateinit var session: MediaSessionCompat
    private var state = PlayerState()
    private var artwork: Bitmap? = null
    private var artworkKey: String? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        api = platformPlayerApi()
        createChannel()
        session = MediaSessionCompat(this, "PrivateMusic").apply {
            setCallback(object : MediaSessionCompat.Callback() {
                override fun onPlay() = command { api.toggle() }
                override fun onPause() = command { api.toggle() }
                override fun onSkipToNext() = command { api.next() }
                override fun onSkipToPrevious() = command { api.previous() }
                override fun onSeekTo(position: Long) = command { api.seek(position / 1000.0) }
            })
            isActive = true
        }
        publish()
        scope.launch {
            api.events().collect { raw -> decodeState(raw)?.let { update(it) } }
        }
        scope.launch {
            while (true) {
                runCatching { api.state() }.onSuccess { update(it) }
                delay(500)
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_TOGGLE -> command { api.toggle() }
            ACTION_NEXT -> command { api.next() }
            ACTION_PREVIOUS -> command { api.previous() }
        }
        publish()
        return START_STICKY
    }

    private fun command(action: suspend () -> Unit) {
        scope.launch {
            runCatching { action() }
            runCatching { api.state() }.onSuccess { update(it) }
        }
    }

    private fun decodeState(raw: String): PlayerState? {
        val body = runCatching { json.parseToJsonElement(raw) }.getOrNull() as? JsonObject ?: return null
        if (!body.isPlayerStateEvent()) return null
        return runCatching { json.decodeFromJsonElement(PlayerState.serializer(), body) }.getOrNull()
    }

    private fun update(next: PlayerState) {
        val oldTrack = state.track
        state = next
        if (oldTrack?.artworkKey() != next.track?.artworkKey()) {
            artwork = null
            artworkKey = next.track?.artworkKey()
            next.track?.artwork_url?.let { loadArtwork(it, artworkKey) }
        }
        publish()
    }

    private fun loadArtwork(url: String, key: String?) {
        scope.launch {
            val bitmap = withContext(Dispatchers.IO) {
                runCatching {
                    val connection = URL(url).openConnection().apply {
                        connectTimeout = 5_000
                        readTimeout = 5_000
                    }
                    connection.getInputStream().use(BitmapFactory::decodeStream)
                }.getOrNull()
            }
            if (bitmap != null && state.track?.artworkKey() == key) {
                artwork = bitmap
                publish()
            }
        }
    }

    private fun publish() {
        if (!::session.isInitialized) return
        val track = state.track
        val playing = state.status == "playing"
        val duration = state.duration.coerceAtLeast(0.0).toLong()
        val position = state.position.coerceIn(0.0, state.duration.coerceAtLeast(state.position)).toLong()
        session.setMetadata(MediaMetadataCompat.Builder()
            .putString(MediaMetadataCompat.METADATA_KEY_TITLE, track?.title ?: "Private Music")
            .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, track?.artist.orEmpty())
            .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, track?.album_title.orEmpty())
            .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, duration * 1000)
            .putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, artwork)
            .build())
        session.setPlaybackState(PlaybackStateCompat.Builder()
            .setActions(
                PlaybackStateCompat.ACTION_PLAY or PlaybackStateCompat.ACTION_PAUSE or
                    PlaybackStateCompat.ACTION_PLAY_PAUSE or PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
                    PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or PlaybackStateCompat.ACTION_SEEK_TO,
            )
            .setState(
                if (playing) PlaybackStateCompat.STATE_PLAYING else if (track != null) PlaybackStateCompat.STATE_PAUSED else PlaybackStateCompat.STATE_NONE,
                position * 1000,
                if (playing) 1f else 0f,
            )
            .build())
        val notification = notification(track, playing, duration, position)
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            notification,
            if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0,
        )
    }

    private fun notification(track: Track?, playing: Boolean, duration: Long, position: Long): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(track?.title ?: "Private Music")
            .setContentText(listOfNotNull(track?.artist?.takeIf { it.isNotBlank() }, "${time(position)} / ${time(duration)}").joinToString(" • "))
            .setLargeIcon(artwork)
            .setContentIntent(open)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOnlyAlertOnce(true)
            .setOngoing(playing)
            .setProgress((duration * 1000).toInt(), (position * 1000).toInt(), duration <= 0)
            .addAction(android.R.drawable.ic_media_previous, "Предыдущий", action(ACTION_PREVIOUS, 1))
            .addAction(if (playing) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play, if (playing) "Пауза" else "Играть", action(ACTION_TOGGLE, 2))
            .addAction(android.R.drawable.ic_media_next, "Следующий", action(ACTION_NEXT, 3))
            .setStyle(androidx.media.app.NotificationCompat.MediaStyle()
                .setMediaSession(session.sessionToken)
                .setShowActionsInCompactView(0, 1, 2))
            .build()
    }

    private fun action(name: String, request: Int): PendingIntent = PendingIntent.getService(
        this, request, Intent(this, PlaybackService::class.java).setAction(name),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun createChannel() {
        if (Build.VERSION.SDK_INT >= 26) {
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(NotificationChannel(CHANNEL_ID, "Воспроизведение", NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            })
        }
    }

    override fun onDestroy() {
        session.release()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        scope.cancel()
        super.onDestroy()
    }

    private fun Track.artworkKey(): String = artwork_url ?: key
    private fun time(seconds: Long): String = "%d:%02d".format(seconds / 60, seconds % 60)

    companion object {
        private const val CHANNEL_ID = "private_music_playback"
        private const val NOTIFICATION_ID = 41
        const val ACTION_TOGGLE = "com.dec.privatemusic.action.TOGGLE"
        const val ACTION_NEXT = "com.dec.privatemusic.action.NEXT"
        const val ACTION_PREVIOUS = "com.dec.privatemusic.action.PREVIOUS"
    }
}
