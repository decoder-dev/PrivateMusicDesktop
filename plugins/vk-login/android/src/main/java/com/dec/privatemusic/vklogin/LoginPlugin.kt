package com.dec.privatemusic.vklogin

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.ConnectivityManager
import android.content.pm.ApplicationInfo
import android.util.Log
import android.provider.OpenableColumns
import java.io.File
import java.util.UUID
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class InstallUpdateArgs { lateinit var path: String }

@InvokeArg
class StartArgs {
    /** Native channel to Rust: every [LoginEvent] goes here, never to JavaScript. */
    lateinit var onEvent: Channel
}

@InvokeArg
class AudioShareArgs {
    lateinit var path: String
    var title: String = ""
    var artist: String = ""
    var album: String = ""
    var lyricsText: String = ""
    var lyricsLrc: String = ""
    var artworkPath: String? = null
}

@InvokeArg
class ClipboardTextArgs { lateinit var text: String }

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
        CrashReport.install(activity)
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
    fun installUpdate(invoke: Invoke) {
        val args = invoke.parseArgs(InstallUpdateArgs::class.java)
        try {
            val apk = File(args.path).canonicalFile
            val directory = File(activity.cacheDir, "updates").canonicalFile
            require(apk.parentFile == directory && apk.isFile && UpdatePolicy.validName(apk.name))
            val pm = activity.packageManager
            @Suppress("DEPRECATION")
            val flags = if (android.os.Build.VERSION.SDK_INT >= 28) android.content.pm.PackageManager.GET_SIGNING_CERTIFICATES else android.content.pm.PackageManager.GET_SIGNATURES
            @Suppress("DEPRECATION")
            val archive = pm.getPackageArchiveInfo(apk.path, flags) ?: error("invalid APK")
            @Suppress("DEPRECATION")
            val current = pm.getPackageInfo(activity.packageName, flags)
            @Suppress("DEPRECATION")
            fun version(info: android.content.pm.PackageInfo): Long = if (android.os.Build.VERSION.SDK_INT >= 28) info.longVersionCode else info.versionCode.toLong()
            @Suppress("DEPRECATION")
            fun signers(info: android.content.pm.PackageInfo): Set<String> {
                val certificates = if (android.os.Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures
                return certificates.orEmpty().map { signature ->
                    java.security.MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).joinToString("") { "%02x".format(it) }
                }.toSet()
            }
            require(UpdatePolicy.accepts(activity.packageName, archive.packageName, version(current), version(archive), signers(current), signers(archive)))
            activity.runOnUiThread {
                try {
                    if (android.os.Build.VERSION.SDK_INT >= 26 && !pm.canRequestPackageInstalls()) {
                        activity.startActivity(Intent(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, android.net.Uri.parse("package:${activity.packageName}")))
                        invoke.reject("install_permission")
                    } else {
                        val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", apk)
                        val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        intent.clipData = ClipData.newRawUri("Private Music update", uri)
                        activity.startActivity(intent)
                        invoke.resolve()
                    }
                } catch (_: Exception) { invoke.reject("installer") }
            }
        } catch (_: Exception) { invoke.reject("installer") }
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

    @Command
    fun coverSelfTest(invoke: Invoke) {
        if((activity.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) == 0) {invoke.reject("Debug builds only");return}
        try {invoke.resolve(JSObject().put("status",CoverImageSelfTest.run(activity.cacheDir)))}
        catch(error: Exception) {invoke.reject(error.message ?: "Cover image self-test failed")}
    }

    private val coverPickerRunning = java.util.concurrent.atomic.AtomicBoolean(false)

    @Command
    fun pickCover(invoke: Invoke) {
        if(!coverPickerRunning.compareAndSet(false,true)) {invoke.reject("An image picker is already open");return}
        try {
            val intent=Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type="image/*"
                putExtra(Intent.EXTRA_MIME_TYPES,arrayOf("image/jpeg","image/png","image/webp"))
            }
            startActivityForResult(invoke,intent,"onCoverPicked")
        } catch(_: Exception) {coverPickerRunning.set(false);invoke.reject("Could not open the image picker")}
    }

    @ActivityCallback
    fun onCoverPicked(invoke: Invoke,result: ActivityResult) {
        val uri=result.data?.data
        if(result.resultCode!=Activity.RESULT_OK || uri==null) {coverPickerRunning.set(false);invoke.resolve(JSObject().put("cancelled",true));return}
        Thread({
            try {
                require(uri.scheme=="content")
                val bytes=activity.contentResolver.openInputStream(uri)?.use {CoverImagePolicy.read(it)} ?: error("Image unavailable")
                val jpeg=CoverImageDecoder.jpeg(bytes)
                invoke.resolve(JSObject().put("jpeg",android.util.Base64.encodeToString(jpeg,android.util.Base64.NO_WRAP)))
            } catch(_: Exception) {invoke.reject("Could not read image (JPEG, PNG or WebP, maximum 10 MB)")}
            finally {coverPickerRunning.set(false)}
        },"pm-cover-picker").start()
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
    fun crashReport(invoke: Invoke) {
        try {
            invoke.resolve(JSObject().put("text", CrashReport.text(activity)))
        } catch (_: Exception) { invoke.reject("Crash report unavailable") }
    }

    @Command
    fun networkUsage(invoke: Invoke) {
        try {
            val manager = activity.getSystemService(ConnectivityManager::class.java)
                ?: throw IllegalStateException("Network status unavailable")
            val network = manager.activeNetwork
            val connected = network != null && manager.getNetworkCapabilities(network) != null
            invoke.resolve(JSObject().put("connected", connected).put("metered", manager.isActiveNetworkMetered))
        } catch (_: Exception) { invoke.reject("Network status unavailable") }
    }

    @Command
    fun shareAudio(invoke: Invoke) {
        val args = invoke.parseArgs(AudioShareArgs::class.java)
        shareWorker.execute {
            try {
                val file = AudioSharePolicy.checked(activity.cacheDir, args.path)
                val cover = args.artworkPath?.let { path ->
                    File(path).canonicalFile.also { require(it.parentFile == file.parentFile && it.name == "cover.jpg" && File(path).absoluteFile == it) }
                }
                val files = AudioShareEnricher.prepare(file, args.title, args.artist, args.album, args.lyricsText, args.lyricsLrc, cover)
                val uris = ArrayList(files.map { FileProvider.getUriForFile(activity, "${activity.packageName}.pm.audio.share", it) })
                val intent = Intent(if (uris.size > 1) Intent.ACTION_SEND_MULTIPLE else Intent.ACTION_SEND).apply {
                    type = if (uris.size > 1) "*/*" else if (files[0].extension == "m4a") "audio/mp4" else "audio/mpeg"
                    if (uris.size > 1) putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris) else putExtra(Intent.EXTRA_STREAM, uris[0])
                    putExtra(Intent.EXTRA_SUBJECT, file.nameWithoutExtension)
                    clipData = ClipData.newRawUri(file.name, uris[0]).also { data -> uris.drop(1).forEach { data.addItem(ClipData.Item(it)) } }
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                activity.runOnUiThread {
                    try { activity.startActivity(Intent.createChooser(intent, null).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)); invoke.resolve() }
                    catch (_: Exception) { invoke.reject("Could not share audio") }
                }
            } catch (_: Exception) { invoke.reject("Could not prepare audio") }
        }
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
    fun copyText(invoke: Invoke) {
        val args = invoke.parseArgs(ClipboardTextArgs::class.java)
        if (!ClipboardTextPolicy.accepts(args.text)) { invoke.reject("Invalid clipboard text"); return }
        try {
            val clipboard = activity.getSystemService(ClipboardManager::class.java) ?: throw IllegalStateException("Clipboard unavailable")
            clipboard.setPrimaryClip(ClipData.newPlainText("Private Music", args.text))
            invoke.resolve()
        } catch (_: Exception) { invoke.reject("Could not copy text") }
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
        val shareWorker = java.util.concurrent.Executors.newSingleThreadExecutor()
    }
}
