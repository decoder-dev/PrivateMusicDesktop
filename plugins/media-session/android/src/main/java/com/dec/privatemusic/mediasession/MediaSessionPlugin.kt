package com.dec.privatemusic.mediasession

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import android.webkit.WebView
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class EventChannelArgs {
    lateinit var channel: Channel
}

/**
 * Tauri side of the media session: parses commands from Rust and hands them to
 * [MediaHub] on the main thread. Holds no playback state itself, because the
 * Activity (and with it this object) can be destroyed while the service plays on.
 */
@TauriPlugin
class MediaSessionPlugin(private val activity: Activity) : Plugin(activity) {
    private val main = Handler(Looper.getMainLooper())
    private var resumed = false

    override fun load(webView: WebView) {
        MediaHub.attach(activity.applicationContext)
    }

    @Suppress("OVERRIDE_DEPRECATION") // the AppCompatActivity overloads need appcompat on our classpath
    override fun onResume() {
        resumed = true
        // The user may have granted the permission since the last update: show the notification.
        PlaybackService.instance?.refresh()
    }

    @Suppress("OVERRIDE_DEPRECATION")
    override fun onPause() {
        resumed = false
    }

    @Command
    fun setEventChannel(invoke: Invoke) {
        val args = invoke.parseArgs(EventChannelArgs::class.java)
        MediaHub.setChannel(args.channel)
        invoke.resolve()
    }

    @Command
    fun update(invoke: Invoke) {
        val parsed = try {
            Wire.parseUpdate(invoke.getArgs())
        } catch (e: Exception) {
            invoke.reject("invalid update")
            return
        }
        main.post {
            MediaHub.onUpdate(parsed)
            if (parsed.playback?.kind == PlaybackKind.PLAYING) askForNotifications()
        }
        invoke.resolve()
    }

    @Command
    fun configurePlayback(invoke: Invoke) {
        val args = invoke.getArgs()
        val resume = args.optBoolean("resumeBluetooth", true)
        val pause = args.optBoolean("pauseAtZero", false)
        main.post { MediaHub.configurePlayback(resume, pause) }
        invoke.resolve()
    }

    @Command
    fun setArtworkUrl(invoke: Invoke) {
        val url = invoke.getArgs().let { if (it.isNull("url")) null else it.optString("url", "") }
        main.post { MediaHub.setArtworkOverride(url) }
        invoke.resolve()
    }

    @Command
    fun requestAudioFocus(invoke: Invoke) {
        main.post { MediaHub.requestFocus() }
        invoke.resolve()
    }

    @Command
    fun abandonAudioFocus(invoke: Invoke) {
        main.post { MediaHub.abandonFocus() }
        invoke.resolve()
    }

    @Command
    fun stop(invoke: Invoke) {
        main.post { MediaHub.stop() }
        invoke.resolve()
    }

    @Command
    fun status(invoke: Invoke) {
        val power = activity.getSystemService(Context.POWER_SERVICE) as? PowerManager
        val result = JSObject()
        result.put("notificationsEnabled", notificationsEnabled())
        result.put("ignoringBatteryOptimizations", power?.isIgnoringBatteryOptimizations(activity.packageName) == true)
        invoke.resolve(result)
    }

    /** Opens the system list of battery-optimisation exemptions. Never requests one itself. */
    @Command
    fun openBatterySettings(invoke: Invoke) {
        val intent = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val fallback = Intent(Settings.ACTION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            try {
                activity.startActivity(intent)
            } catch (_: ActivityNotFoundException) {
                activity.startActivity(fallback)
            }
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("could not open the battery settings")
        }
    }

    private fun notificationsGranted(): Boolean =
        Build.VERSION.SDK_INT < PermissionPolicy.TIRAMISU ||
            ContextCompat.checkSelfPermission(activity, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun notificationsEnabled(): Boolean = PermissionPolicy.notificationsEnabled(
        Build.VERSION.SDK_INT,
        notificationsGranted(),
        NotificationManagerCompat.from(activity).areNotificationsEnabled(),
    )

    /**
     * First time playback starts on Android 13+: ask once. A "no" only hides the
     * notification; the foreground service and the sound are unaffected.
     */
    private fun askForNotifications() {
        val prefs = activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (!resumed || !PermissionPolicy.shouldAskForNotifications(
                Build.VERSION.SDK_INT, notificationsGranted(), prefs.getBoolean(KEY_ASKED, false),
            )
        ) {
            return
        }
        prefs.edit().putBoolean(KEY_ASKED, true).apply()
        try {
            ActivityCompat.requestPermissions(activity, arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
        } catch (e: RuntimeException) {
            Log.w(TAG, "notification permission request failed: ${e.javaClass.simpleName}")
        }
    }

    private companion object {
        const val TAG = "MediaSession"
        const val PREFS = "media_session"
        const val KEY_ASKED = "asked_notifications"
        const val REQUEST_NOTIFICATIONS = 0x504D
    }
}
