package com.dec.privatemusic.player

private val sharedTransport by lazy { JniTransport() }
private val sharedApi by lazy { RustPlayerApi(sharedTransport) }

private class JniTransport : RustTransport {
    private val pointer = RustPlayerApiJni.pm_engine_new(AndroidPlayerRuntime.dataPath())

    override fun command(raw: String): String = RustPlayerApiJni.pm_engine_command(pointer, raw)
    override fun poll(): String? = RustPlayerApiJni.pm_engine_poll_event(pointer)
    override fun free() = RustPlayerApiJni.pm_engine_free(pointer)
}

private val sharedPlayer by lazy {
    object : PlayerApi by sharedApi {
        // The Android process shares one engine with PlaybackService and survives activity recreation.
        override fun close() = Unit
    }
}

actual fun platformPlayerApi(): PlayerApi = sharedPlayer
actual fun platformSignIn(session: Session) = sharedApi.signIn(session)
actual fun platformSignOut() = sharedApi.signOut()
actual fun platformSessionState(): SessionState = sharedApi.sessionState()
actual fun platformCatalogApi(): CatalogApi = sharedApi

object RustPlayerApiJni {
    init {
        System.loadLibrary("private_music_engine")
    }

    @JvmStatic external fun pm_engine_new(dataDir: String): Long
    @JvmStatic external fun pm_engine_free(pointer: Long)
    @JvmStatic external fun pm_engine_command(pointer: Long, raw: String): String
    @JvmStatic external fun pm_engine_poll_event(pointer: Long): String?
}
