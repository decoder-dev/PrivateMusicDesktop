package com.dec.privatemusic.player

private class JniTransport : RustTransport {
    private val pointer = RustPlayerApiJni.pm_engine_new()

    override fun command(raw: String): String = RustPlayerApiJni.pm_engine_command(pointer, raw)
    override fun poll(): String? = RustPlayerApiJni.pm_engine_poll_event(pointer)
    override fun free() = RustPlayerApiJni.pm_engine_free(pointer)
}

actual fun platformPlayerApi(): PlayerApi = RustPlayerApi(JniTransport())

object RustPlayerApiJni {
    init {
        System.loadLibrary("private_music_engine")
    }

    @JvmStatic external fun pm_engine_new(): Long
    @JvmStatic external fun pm_engine_free(pointer: Long)
    @JvmStatic external fun pm_engine_command(pointer: Long, raw: String): String
    @JvmStatic external fun pm_engine_poll_event(pointer: Long): String?
}
