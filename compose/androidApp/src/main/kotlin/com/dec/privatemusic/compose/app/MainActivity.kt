package com.dec.privatemusic.compose.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.core.content.ContextCompat
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.remember
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.SessionStore
import com.dec.privatemusic.player.SessionViewModel
import com.dec.privatemusic.player.platformPlayerApi
import com.dec.privatemusic.ui.PrivateMusicApp

class MainActivity : ComponentActivity() {
    private lateinit var sessions: SessionStore
    private lateinit var sessionViewModel: SessionViewModel
    private val loginLauncher = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode == RESULT_OK) {
            result.data?.getStringExtra("session")?.let {
                sessions.saveJson(it)
                sessions.sync()
                sessionViewModel.refresh()
            }
        }
    }

    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        sessions = SessionStore(this)
        sessionViewModel = SessionViewModel()
        ContextCompat.startForegroundService(this, Intent(this, PlaybackService::class.java))
        if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        enableEdgeToEdge()
        setContent {
            val api = remember { platformPlayerApi() }
            val player = remember(api) { PlayerViewModel(api) }
            PrivateMusicApp(
                viewModel = player,
                sessionViewModel = sessionViewModel,
                onLogin = { loginLauncher.launch(Intent(this, VkLoginActivity::class.java)) },
                onLogout = {
                    sessions.clear()
                    sessionViewModel.refresh()
                },
                bindBackHandler = { enabled, onBack -> BackHandler(enabled, onBack) },
            )
        }
    }
}
