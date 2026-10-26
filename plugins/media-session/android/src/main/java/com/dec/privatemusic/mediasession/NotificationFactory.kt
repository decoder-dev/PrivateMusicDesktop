package com.dec.privatemusic.mediasession

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.support.v4.media.session.MediaSessionCompat
import androidx.core.app.NotificationCompat
import androidx.media.app.NotificationCompat.MediaStyle

/** Builds the playback notification. The only code that knows channel and icon names. */
class NotificationFactory(private val context: Context) {

    fun ensureChannel() {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        val channel = NotificationChannel(CHANNEL_ID, "Playback", NotificationManager.IMPORTANCE_LOW).apply {
            description = "Controls for the music that is playing"
            setSound(null, null)
            enableVibration(false)
            setShowBadge(false)
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
        }
        manager.createNotificationChannel(channel)
    }

    fun build(
        token: MediaSessionCompat.Token,
        track: TrackInfo?,
        kind: PlaybackKind,
        artwork: Bitmap?,
        foreground: Boolean,
        canNext: Boolean = true,
    ): Notification {
        ensureChannel()
        val buttons = NotificationLayout.buttons(kind, canNext)
        val style = MediaStyle().setMediaSession(token)
        if (buttons.isNotEmpty()) style.setShowActionsInCompactView(*NotificationLayout.compactIndices(kind, canNext))
        val builder = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_music)
            .setContentTitle(track?.title?.ifEmpty { null } ?: appLabel())
            .setContentText(track?.artist)
            .setSubText(track?.album)
            .setLargeIcon(artwork)
            .setContentIntent(contentIntent())
            .setDeleteIntent(servicePending(PlaybackService.ACTION_DISMISSED, REQUEST_DISMISS))
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setShowWhen(false)
            .setOngoing(foreground && NotificationLayout.isOngoing(kind))
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setStyle(style)
        for (button in buttons) {
            val (icon, label) = iconAndLabel(button)
            builder.addAction(icon, label, servicePending(button.intentAction, button.ordinal + 1))
        }
        if (artwork != null) {
            builder.setColorized(true).setColor(NotificationColor.forBackground(averageColor(artwork)))
        }
        return builder.build()
    }

    private fun iconAndLabel(button: ButtonId): Pair<Int, String> = when (button) {
        ButtonId.PREVIOUS -> android.R.drawable.ic_media_previous to "Previous"
        ButtonId.PLAY -> android.R.drawable.ic_media_play to "Play"
        ButtonId.PAUSE -> android.R.drawable.ic_media_pause to "Pause"
        ButtonId.NEXT -> android.R.drawable.ic_media_next to "Next"
        ButtonId.STOP -> android.R.drawable.ic_menu_close_clear_cancel to "Stop"
    }

    private fun averageColor(bitmap: Bitmap): Int = try {
        Bitmap.createScaledBitmap(bitmap, 1, 1, true).let { pixel ->
            pixel.getPixel(0, 0).also { if (pixel !== bitmap) pixel.recycle() }
        }
    } catch (_: RuntimeException) {
        Color.DKGRAY
    }

    private fun appLabel(): String = context.applicationInfo.loadLabel(context.packageManager).toString()

    private fun contentIntent(): PendingIntent? {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return null
        launch.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        return PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    private fun servicePending(action: String, requestCode: Int): PendingIntent =
        PendingIntent.getService(
            context,
            requestCode,
            Intent(context, PlaybackService::class.java).setAction(action),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    companion object {
        const val CHANNEL_ID = "playback"
        private const val REQUEST_DISMISS = 100
    }
}
