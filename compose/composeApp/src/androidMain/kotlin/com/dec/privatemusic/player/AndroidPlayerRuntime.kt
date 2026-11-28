package com.dec.privatemusic.player

import android.content.Context
import java.io.File

/** Supplies the application context and writable sandbox to the native engine. */
object AndroidPlayerRuntime {
    private lateinit var application: Context

    fun initialize(context: Context) {
        application = context.applicationContext
    }

    internal fun applicationContext(): Context = application

    internal fun dataPath(): String = File(application.filesDir, "native-engine").absolutePath
}
