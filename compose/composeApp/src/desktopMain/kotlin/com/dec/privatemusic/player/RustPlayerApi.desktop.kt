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

private object JnaEngine : NativeEngine by Native.load("private_music_engine", NativeEngine::class.java)
private val sharedTransport by lazy { JnaTransport() }
private val sharedApi by lazy { RustPlayerApi(sharedTransport) }

private class JnaTransport : RustTransport {
    private val pointer = JnaEngine.pm_engine_new()

    override fun command(raw: String): String = read { JnaEngine.pm_engine_command(pointer, raw) }.also { response ->
        if (System.getProperty("pm.compose.trace") == "1") println("Native command: $raw; response: $response")
    }
    override fun poll(): String? = JnaEngine.pm_engine_poll_event(pointer)?.let { pointer ->
        pointer.getString(0).also { JnaEngine.pm_string_free(pointer) }
    }

    private fun read(call: () -> Pointer?): String {
        val result = call() ?: throw PlayerException("Null engine response")
        return result.getString(0).also { JnaEngine.pm_string_free(result) }
    }

    override fun free() = JnaEngine.pm_engine_free(pointer)
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
