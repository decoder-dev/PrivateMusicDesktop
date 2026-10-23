package com.dec.privatemusic.vklogin

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.util.Log
import android.provider.OpenableColumns
import java.io.File
import java.util.UUID
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class StartArgs {
    /** Native channel to Rust: every [LoginEvent] goes here, never to JavaScript. */
    lateinit var onEvent: Channel
}

@InvokeArg
class ShareArgs {
    lateinit var url: String
    var title: String = ""
}

/**
 * Rust-facing half. `start` opens [LoginActivity] with startActivityForResult;
 * the invoke is answered when the Activity is gone. The events (including the
 * session) are not in that answer: [LoginActivity] sends them through
 * [LoginBridge] to the channel. See README.md, "Process death".
 */
@TauriPlugin
class LoginPlugin(private val activity: Activity) : Plugin(activity) {
    override fun load(webView: WebView) {
        val debuggable = (activity.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        val extras = activity.intent
        if (SmokeMode.requested(
                extras?.getBooleanExtra(SmokeMode.EXTRA, false) ?: false,
                extras?.getStringExtra(SmokeMode.EXTRA),
                debuggable,
            ) && !LoginBridge.live
        ) {
            Log.i(TAG, "smoke mode: opening the sign-in screen")
            LoginBridge.live = true
            activity.startActivity(Intent(activity, LoginActivity::class.java).putExtra(SmokeMode.EXTRA, true))
        }
    }

    @Command
    fun start(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)
        if (LoginBridge.live) {
            // Already on screen: bring it back and have it check the session now.
            LoginBridge.recheck?.invoke()
            activity.startActivity(
                Intent(activity, LoginActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                    .putExtra(LoginActivity.EXTRA_MANUAL, true),
            )
            invoke.resolve(JSObject().put("status", "running"))
            return
        }
        val channel = args.onEvent
        LoginBridge.sink = { event ->
            try {
                channel.send(JSObject(event.toJson().toString()))
            } catch (e: Exception) {
                Log.w(TAG, "could not deliver an event: ${e.javaClass.simpleName}")
            }
        }
        LoginBridge.live = true
        try {
            startActivityForResult(invoke, Intent(activity, LoginActivity::class.java), "onLoginResult")
        } catch (e: Exception) {
            LoginBridge.sink = null
            LoginBridge.live = false
            invoke.reject("could not open the sign-in screen")
        }
    }

    @ActivityCallback
    fun onLoginResult(invoke: Invoke, result: ActivityResult) {
        // Nothing to clear here: the Activity clears the bridge as it is destroyed, and
        // a new start() may already have installed its own sink.
        val reply = JSObject().put("status", "finished")
        result.data?.getStringExtra(LoginActivity.EXTRA_SMOKE_RESULT)?.let { reply.put("smoke", it) }
        invoke.resolve(reply)
    }

    @Command
    fun cancel(invoke: Invoke) {
        LoginBridge.cancelRequest?.invoke()
        invoke.resolve()
    }

    private val audioPickerRunning = java.util.concurrent.atomic.AtomicBoolean(false)

    @Command
    fun pickAudio(invoke: Invoke) {
        if (!audioPickerRunning.compareAndSet(false, true)) { invoke.reject("An audio picker is already open"); return }
        try {
            val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "audio/*"
                putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("audio/mpeg", "audio/mp3"))
            }
            startActivityForResult(invoke, intent, "onAudioPicked")
        } catch (_: Exception) {
            audioPickerRunning.set(false)
            invoke.reject("Could not open the audio picker")
        }
    }

    @ActivityCallback
    fun onAudioPicked(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || uri == null) {
            audioPickerRunning.set(false)
            invoke.resolve(JSObject().put("cancelled", true))
            return
        }
        Thread({
            var staged: File? = null
            try {
                if (uri.scheme != "content") throw java.io.IOException("Unsupported audio provider")
                var name = "audio.mp3"
                activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst()) {
                        val nameColumn = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        if (nameColumn >= 0) name = cursor.getString(nameColumn)?.take(256) ?: name
                        val sizeColumn = cursor.getColumnIndex(OpenableColumns.SIZE)
                        if (sizeColumn >= 0 && !cursor.isNull(sizeColumn) && cursor.getLong(sizeColumn) > AudioFilePolicy.MAX_BYTES)
                            throw java.io.IOException("Audio exceeds size limit")
                    }
                }
                val folder = File(activity.cacheDir, "audio-picker").apply { mkdirs() }
                // These are only our private temporary files, left by interrupted pickers.
                folder.listFiles()?.filter { it.name.startsWith("pick-") && it.lastModified() < System.currentTimeMillis() - 86_400_000L }
                    ?.forEach { it.delete() }
                val target = File(folder, "pick-${UUID.randomUUID()}.mp3")
                staged = target
                val bytes = activity.contentResolver.openInputStream(uri)?.use { input ->
                    target.outputStream().use { output -> AudioFilePolicy.copyMp3(input, output) }
                } ?: throw java.io.IOException("Audio could not be read")
                invoke.resolve(JSObject().put("path", target.absolutePath).put("name", name).put("bytes", bytes))
            } catch (_: Exception) {
                staged?.delete()
                invoke.reject("Could not read MP3 (maximum 200 MB)")
            } finally {
                audioPickerRunning.set(false)
            }
        }, "pm-audio-picker").start()
    }

    @Command
    fun shareLink(invoke: Invoke) {
        val args = invoke.parseArgs(ShareArgs::class.java)
        if (!ShareLinkPolicy.accepts(args.url)) { invoke.reject("Invalid VK share link"); return }
        try {
            val intent = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_TEXT, args.url)
                putExtra(Intent.EXTRA_SUBJECT, args.title.take(200))
            }
            activity.startActivity(Intent.createChooser(intent, null))
            invoke.resolve()
        } catch (_: Exception) { invoke.reject("Could not open sharing") }
    }

    @Command
    fun copyLink(invoke: Invoke) {
        val args = invoke.parseArgs(ShareArgs::class.java)
        if (!ShareLinkPolicy.accepts(args.url)) { invoke.reject("Invalid VK share link"); return }
        try {
            val clipboard = activity.getSystemService(ClipboardManager::class.java) ?: throw IllegalStateException("Clipboard unavailable")
            clipboard.setPrimaryClip(ClipData.newPlainText("Private Music", args.url))
            invoke.resolve()
        } catch (_: Exception) { invoke.reject("Could not copy the link") }
    }

    private companion object {
        const val TAG = "VkLogin"
    }
}
