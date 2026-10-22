package com.dec.privatemusic.vklogin

import android.app.Activity
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.util.Log
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

    private companion object {
        const val TAG = "VkLogin"
    }
}
