package com.dec.privatemusic.vklogin

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Debug
import android.os.Build
import android.util.Log
import android.webkit.WebView
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * What the owner needs when the app "just closes": a short text with the device, the WebView version, the
 * reasons the system gave for the last process deaths (Android 11+: crash, native crash with its signal, ANR,
 * low memory) and the stack of the last uncaught Java exception. Nothing is sent anywhere; the app shows it and
 * the user copies it.
 */
/**
 * A short, secret-free trail of the sign-in (hosts, cookie NAMES, VK's answer type), kept on the device and shown in
 * the report: when sign-in "does nothing", this says where it stopped. The sink is set by [CrashReport.install];
 * without it (JVM unit tests) every call is a no-op.
 */
object LoginTrace {
    @Volatile var sink: ((String) -> Unit)? = null
    fun note(message: String) { sink?.invoke(message) }
}

object CrashReport {
    private const val FILE = "pm-java-crash.txt"
    private const val TRACE_FILE = "pm-login-trace.txt"
    private const val MAX_TRACE_FILE = 12_000
    private const val MAX_FILE_BYTES = 24_000
    private const val MAX_TRACE_CHARS = 6_000
    private const val MAIN_THREAD_LINES = 60

    /** Writes every uncaught Java exception to a file before the process dies. */
    fun install(context: Context) {
        val app = context.applicationContext
        LoginTrace.sink = { message ->
            try {
                val file = File(app.filesDir, TRACE_FILE)
                val old = if (file.exists()) file.readText() else ""
                file.writeText((old + stamp(System.currentTimeMillis()) + " " + message.take(300) + "\n").takeLast(MAX_TRACE_FILE))
            } catch (_: Throwable) { /* diagnostics must never break sign-in */ }
        }
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            try {
                val file = File(app.filesDir, FILE)
                val entry = "${stamp(System.currentTimeMillis())} thread=${thread.name}\n${Log.getStackTraceString(error)}\n"
                val old = if (file.exists()) file.readText().takeLast(MAX_FILE_BYTES) else ""
                file.writeText((old + entry).takeLast(MAX_FILE_BYTES))
            } catch (_: Throwable) {
                // The crash itself is what matters; never throw from the handler.
            }
            previous?.uncaughtException(thread, error)
        }
    }

    fun text(context: Context): String {
        val out = StringBuilder()
        out.append("Private Music diagnostic report\n")
        out.append("time: ").append(stamp(System.currentTimeMillis())).append('\n')
        try {
            val info = context.packageManager.getPackageInfo(context.packageName, 0)
            out.append("app: ").append(info.versionName).append(" (").append(info.longVersionCode).append(")\n")
        } catch (_: Exception) { out.append("app: unknown\n") }
        out.append("device: ").append(Build.MANUFACTURER).append(' ').append(Build.MODEL)
            .append(", Android ").append(Build.VERSION.RELEASE).append(" (API ").append(Build.VERSION.SDK_INT).append(")")
            .append(", ").append(Build.SUPPORTED_ABIS.joinToString("/")).append('\n')
        out.append("webview: ").append(webViewVersion()).append("\n\n")
        try {
            val memory = Debug.MemoryInfo()
            Debug.getMemoryInfo(memory)
            out.append("memory now: pss_kb=").append(memory.totalPss)
                .append(" java_used_bytes=").append(Runtime.getRuntime().totalMemory() - Runtime.getRuntime().freeMemory())
                .append(" java_limit_bytes=").append(Runtime.getRuntime().maxMemory()).append("\n\n")
        } catch (_: Exception) { out.append("memory now: unavailable\n\n") }
        if (Build.VERSION.SDK_INT >= 30) out.append(exitReasons(context))
        else out.append("process exit reasons: need Android 11 or newer\n")
        val trace = File(context.filesDir, TRACE_FILE)
        if (trace.exists()) out.append("\n== sign-in trail ==\n").append(trace.readText().takeLast(MAX_TRACE_FILE))
        val file = File(context.filesDir, FILE)
        if (file.exists()) out.append("\n== uncaught Java exceptions ==\n").append(file.readText().takeLast(MAX_FILE_BYTES))
        return out.toString()
    }

    private fun webViewVersion(): String = try {
        val pkg = if (Build.VERSION.SDK_INT >= 26) WebView.getCurrentWebViewPackage() else null
        if (pkg == null) "unknown" else "${pkg.packageName} ${pkg.versionName}"
    } catch (_: Throwable) { "unavailable" }

    private fun stamp(millis: Long) = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(millis))

    private fun reasonName(reason: Int): String = when (reason) {
        ApplicationExitInfo.REASON_ANR -> "ANR (not responding)"
        ApplicationExitInfo.REASON_CRASH -> "CRASH (Java exception)"
        ApplicationExitInfo.REASON_CRASH_NATIVE -> "CRASH_NATIVE (native signal)"
        ApplicationExitInfo.REASON_EXIT_SELF -> "EXIT_SELF"
        ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "INITIALIZATION_FAILURE"
        ApplicationExitInfo.REASON_LOW_MEMORY -> "LOW_MEMORY (killed by the system)"
        ApplicationExitInfo.REASON_PERMISSION_CHANGE -> "PERMISSION_CHANGE"
        ApplicationExitInfo.REASON_SIGNALED -> "SIGNALED (killed)"
        ApplicationExitInfo.REASON_USER_REQUESTED -> "USER_REQUESTED (force stop)"
        ApplicationExitInfo.REASON_USER_STOPPED -> "USER_STOPPED"
        ApplicationExitInfo.REASON_DEPENDENCY_DIED -> "DEPENDENCY_DIED"
        ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE -> "EXCESSIVE_RESOURCE_USAGE"
        ApplicationExitInfo.REASON_FREEZER -> "FREEZER"
        ApplicationExitInfo.REASON_UNKNOWN -> "UNKNOWN (not specified by the system)"
        ApplicationExitInfo.REASON_OTHER -> "OTHER (system decision)"
        ApplicationExitInfo.REASON_PACKAGE_STATE_CHANGE -> "PACKAGE_STATE_CHANGE"
        ApplicationExitInfo.REASON_PACKAGE_UPDATED -> "PACKAGE_UPDATED (app update)"
        else -> "UNKNOWN($reason)"
    }

    private fun exitReasons(context: Context): String {
        val out = StringBuilder("== how the app process ended (newest first) ==\n")
        try {
            val manager = context.getSystemService(ActivityManager::class.java)
            val list = manager.getHistoricalProcessExitReasons(context.packageName, 0, 8)
            if (list.isEmpty()) out.append("(nothing recorded)\n")
            for (item in list) {
                out.append(stamp(item.timestamp)).append("  ").append(reasonName(item.reason))
                    .append("  process=").append(item.processName).append("  status=").append(item.status)
                    .append("  importance=").append(item.importance)
                if (item.pss > 0) out.append("  pss_kb=").append(item.pss)
                if (item.rss > 0) out.append("  rss_kb=").append(item.rss)
                out.append('\n')
                if (!item.description.isNullOrBlank()) out.append("  ").append(item.description).append('\n')
                // An ANR trace is plain text; a native crash trace is binary protobuf, so only its description is used.
                if (item.reason == ApplicationExitInfo.REASON_ANR) {
                    try {
                        item.traceInputStream?.bufferedReader()?.use { reader ->
                            val text = reader.readText()
                            // The part that matters is the stack of the main thread, not the heap statistics before it.
                            val start = text.indexOf("\"main\" prio=")
                            val part = if (start >= 0) text.substring(start).lines().take(MAIN_THREAD_LINES).joinToString("\n") else text.take(MAX_TRACE_CHARS)
                            out.append(part.take(MAX_TRACE_CHARS)).append('\n')
                        }
                    } catch (_: Exception) { /* no trace kept */ }
                }
            }
        } catch (error: Exception) {
            out.append("unavailable: ").append(error.javaClass.simpleName).append('\n')
        }
        return out.toString()
    }
}
