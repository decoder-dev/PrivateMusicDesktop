package com.dec.privatemusic.compose.app

import android.app.Application
import com.dec.privatemusic.player.AndroidPlayerRuntime
import com.dec.privatemusic.player.SessionStore

class PrivateMusicApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        AndroidPlayerRuntime.initialize(this)
        SessionStore(this).sync()
    }
}
