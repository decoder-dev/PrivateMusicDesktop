package com.dec.privatemusic.player

import android.content.Context
import java.io.File

/** Supplies a writable persistent sandbox to Rust before the singleton engine is created. */
object AndroidPlayerRuntime {
    private var dataDirectory: File? = null

    @Synchronized
    fun initialize(context: Context) {
        if (dataDirectory == null) {
            dataDirectory = File(context.applicationContext.filesDir, "native-engine").apply { mkdirs() }
        }
    }

    internal fun dataPath(): String = checkNotNull(dataDirectory) {
        "AndroidPlayerRuntime must be initialized by the Application"
    }.absolutePath
}
