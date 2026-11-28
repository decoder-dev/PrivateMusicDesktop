package com.dec.privatemusic.desktop

import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.WindowState
import androidx.compose.ui.window.singleWindowApplication
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.SessionViewModel
import com.dec.privatemusic.player.platformPlayerApi
import com.dec.privatemusic.ui.PrivateMusicApp

fun main() = singleWindowApplication(title = "Private Music", state = WindowState(size = DpSize(1080.dp, 720.dp))) {
    val api = remember { platformPlayerApi() }
    val viewModel = remember(api) { PlayerViewModel(api) }
    val session = remember { SessionViewModel() }
    val mediaKeys = remember(api) { DesktopMediaKeys(api) }
    DisposableEffect(mediaKeys) {
        mediaKeys.start()
        onDispose { mediaKeys.close() }
    }
    PrivateMusicApp(
        viewModel = viewModel,
        sessionViewModel = session,
        onLogin = null,
        onLogout = {
            com.dec.privatemusic.player.platformSignOut()
            session.refresh()
        },
    )
}
