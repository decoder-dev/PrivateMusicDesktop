package com.dec.privatemusic.vklogin

/**
 * In-process hand-over between [LoginActivity] and [LoginPlugin]. Events (with
 * the session cookies inside a `Success`) stay in this process's memory and go
 * straight to Rust over the plugin's native channel; they are never put in an
 * Intent, the Activity result, or a web view.
 */
object LoginBridge {
    /** Forwards an event to Rust. Set by [LoginPlugin.start], cleared when the screen ends. */
    @Volatile
    var sink: ((LoginEvent) -> Unit)? = null

    /**
     * A sign-in screen is up in this process. Static on purpose: after process
     * death it is false again, which is how a restored Activity knows nobody is
     * listening for its result any more.
     */
    @Volatile
    var live: Boolean = false

    /** Set by the Activity while it is alive; asks it to finish as cancelled. */
    @Volatile
    var cancelRequest: (() -> Unit)? = null

    /** Set by the Activity while it is alive; "the app asked again": retry a dead page, check the session now. */
    @Volatile
    var recheck: (() -> Unit)? = null

    fun emit(event: LoginEvent) {
        sink?.invoke(event)
    }
}
