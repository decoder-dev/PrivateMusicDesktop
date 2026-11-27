package com.dec.privatemusic.desktop

import androidx.compose.runtime.remember
import androidx.compose.ui.window.singleWindowApplication
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.SessionViewModel
import com.dec.privatemusic.player.platformPlayerApi
import com.dec.privatemusic.ui.PrivateMusicApp

fun main() = singleWindowApplication(title = "Private Music") {
    val api = remember { platformPlayerApi() }
    val viewModel = remember(api) { PlayerViewModel(api) }
    val session = remember { SessionViewModel() }
    PrivateMusicApp(
        viewModel = viewModel,
        sessionViewModel = session,
        onLogin = null,
        onLogout = { session.refresh() },
    )
}
