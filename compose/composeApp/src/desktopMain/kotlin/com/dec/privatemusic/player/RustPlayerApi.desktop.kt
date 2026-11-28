package com.dec.privatemusic.player

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer

private interface NativeEngine : Library {
    fun pm_engine_new(): Pointer
    fun pm_engine_free(pointer: Pointer)
    fun pm_engine_command(pointer: Pointer, raw: String): Pointer?
    fun pm_engine_poll_event(pointer: Pointer): Pointer?
    fun pm_string_free(pointer: Pointer)
}

private const val UTF_8 = "UTF-8"
private object JnaEngine : NativeEngine by Native.load(
    "private_music_engine", NativeEngine::class.java, mapOf(Library.OPTION_STRING_ENCODING to UTF_8),
)
private val sharedTransport by lazy { JnaTransport() }
private val sharedApi by lazy { RustPlayerApi(sharedTransport) }

private class JnaTransport : RustTransport {
    private var engine: Pointer? = JnaEngine.pm_engine_new()

    @Synchronized
    override fun command(raw: String): String {
        val pointer = engine ?: throw PlayerException("Engine is closed")
        val result = JnaEngine.pm_engine_command(pointer, raw) ?: throw PlayerException("Null engine response")
        return read(result)
    }

    @Synchronized
    override fun poll(): String? {
        val pointer = engine ?: return null
        return JnaEngine.pm_engine_poll_event(pointer)?.let(::read)
    }

    private fun read(result: Pointer): String = try {
        result.getString(0, UTF_8)
    } finally {
        JnaEngine.pm_string_free(result)
    }

    @Synchronized
    override fun free() {
        val pointer = engine ?: return
        engine = null
        JnaEngine.pm_engine_free(pointer)
    }
}

actual fun platformPlayerApi(): PlayerApi =
    if (System.getProperty("pm.compose.demo") == "1") DemoPlayerApi() else sharedApi

actual fun platformSignIn(session: Session) {
    if (System.getProperty("pm.compose.demo") != "1") sharedApi.signIn(session)
}
actual fun platformSignOut() {
    if (System.getProperty("pm.compose.demo") != "1") sharedApi.signOut()
}
actual fun platformSessionState(): SessionState =
    if (System.getProperty("pm.compose.demo") == "1") SessionState(signed_in = true, profile = Profile(first_name = "Demo")) else sharedApi.sessionState()
actual fun platformCatalogApi(): CatalogApi =
    if (System.getProperty("pm.compose.demo") == "1") DemoCatalogApi() else sharedApi
