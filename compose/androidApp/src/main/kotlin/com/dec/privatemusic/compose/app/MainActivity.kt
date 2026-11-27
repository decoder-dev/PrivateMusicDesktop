package com.dec.privatemusic.compose.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.remember
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.platformPlayerApi
import com.dec.privatemusic.ui.PrivateMusicApp

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val api = remember { platformPlayerApi() }
            val viewModel = remember(api) { PlayerViewModel(api) }
            PrivateMusicApp(viewModel)
        }
    }
}
