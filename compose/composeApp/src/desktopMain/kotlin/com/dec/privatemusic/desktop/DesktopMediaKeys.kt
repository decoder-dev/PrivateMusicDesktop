package com.dec.privatemusic.desktop

import com.dec.privatemusic.player.PlayerApi
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.Structure
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/** Best-effort global media keys on Windows; Android uses its native MediaSession instead. */
class DesktopMediaKeys(private val api: PlayerApi) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    @Volatile private var running = false
    @Volatile private var nativeThreadId = 0
    private var worker: Thread? = null
    private var user32: HotkeyApi? = null

    fun start() {
        if (running || !System.getProperty("os.name").lowercase(Locale.ROOT).contains("windows")) return
        running = true
        worker = Thread(::runNativeLoop, "private-music-media-keys").apply {
            isDaemon = true
            start()
        }
    }

    private fun runNativeLoop() {
        val native = runCatching { Native.load("user32", HotkeyApi::class.java) }.getOrNull() ?: return
        user32 = native
        nativeThreadId = native.GetCurrentThreadId()
        val registered = HOTKEYS.filter { (id, key) -> native.RegisterHotKey(null, id, MOD_NOREPEAT, key) }.map { it.first }
        val message = HotkeyMessage()
        while (running) {
            val result = native.GetMessageW(message, null, 0, 0)
            if (result <= 0) break
            if (message.message == WM_HOTKEY) dispatch(Pointer.nativeValue(message.wParam).toInt())
        }
        registered.forEach { native.UnregisterHotKey(null, it) }
        user32 = null
        nativeThreadId = 0
    }

    private fun dispatch(id: Int) {
        scope.launch {
            runCatching {
                when (id) {
                    PLAY_PAUSE -> api.toggle()
                    NEXT -> api.next()
                    PREVIOUS -> api.previous()
                }
            }
        }
    }

    fun close() {
        if (!running) return
        running = false
        val native = user32
        val threadId = nativeThreadId
        if (native != null && threadId != 0) native.PostThreadMessageW(threadId, WM_QUIT, null, null)
        worker?.join(1_000)
        worker = null
        scope.cancel()
    }

    private interface HotkeyApi : Library {
        fun RegisterHotKey(window: Pointer?, id: Int, modifiers: Int, key: Int): Boolean
        fun UnregisterHotKey(window: Pointer?, id: Int): Boolean
        fun GetMessageW(message: HotkeyMessage, window: Pointer?, min: Int, max: Int): Int
        fun PostThreadMessageW(thread: Int, message: Int, wParam: Pointer?, lParam: Pointer?): Boolean
        fun GetCurrentThreadId(): Int
    }

    @Structure.FieldOrder("hwnd", "message", "wParam", "lParam", "time", "point")
    class HotkeyMessage : Structure() {
        @JvmField var hwnd: Pointer? = null
        @JvmField var message: Int = 0
        @JvmField var wParam: Pointer? = null
        @JvmField var lParam: Pointer? = null
        @JvmField var time: Int = 0
        @JvmField var point: NativePoint = NativePoint()
    }

    @Structure.FieldOrder("x", "y")
    class NativePoint : Structure() {
        @JvmField var x: Int = 0
        @JvmField var y: Int = 0
    }

    companion object {
        private const val PLAY_PAUSE = 1
        private const val NEXT = 2
        private const val PREVIOUS = 3
        private const val WM_HOTKEY = 0x0312
        private const val WM_QUIT = 0x0012
        private const val MOD_NOREPEAT = 0x4000
        private val HOTKEYS = listOf(PLAY_PAUSE to 0xB3, NEXT to 0xB0, PREVIOUS to 0xB1)
    }
}
