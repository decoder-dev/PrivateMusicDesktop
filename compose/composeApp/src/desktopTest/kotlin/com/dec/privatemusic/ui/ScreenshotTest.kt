package com.dec.privatemusic.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.use
import com.dec.privatemusic.player.*
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.runBlocking
import org.jetbrains.skia.EncodedImageFormat
import org.jetbrains.skia.Image
import kotlin.test.Test
import kotlin.test.assertFalse

@OptIn(ExperimentalComposeUiApi::class)
class ScreenshotTest {
    @Test
    fun homeAndFullPlayerRenderToPng() = runBlocking {
        withDemo {
            renderApp(320, 660, "compact", Offset(150f, 560f))
            renderApp(390, 844, "mobile", Offset(160f, 744f))
            renderApp(1280, 840, "desktop", Offset(380f, 804f))
        }
    }

    private suspend fun renderApp(width: Int, height: Int, layout: String, miniPlayer: Offset) {
        val api = DemoPlayerApi()
        api.playTracks(demoTracks, 1)
        val viewModel = PlayerViewModel(api)
        val session = SessionViewModel()
        try {
            ImageComposeScene(width = width, height = height) {
                PrivateMusicApp(viewModel, session, onLogin = null, onLogout = session::refresh)
            }.use { scene ->
                scene.render()
                val home = save(scene.render(), "$layout-home")
                scene.sendPointerEvent(PointerEventType.Press, miniPlayer, type = PointerType.Touch)
                scene.sendPointerEvent(PointerEventType.Release, miniPlayer, type = PointerType.Touch)
                scene.render()
                val player = save(scene.render(), "$layout-player")
                assertFalse(home.contentEquals(player), "Mini player did not open the player on $layout")
            }
        } finally {
            viewModel.close()
            session.close()
        }
    }

    @Test
    fun catalogScreensRenderToPng() {
        renderScreen("profile") {
            ProfileScreen(SessionState(signed_in = true, profile = Profile(first_name = "Demo", last_name = "Listener")), null, {}, {})
        }
        catalogScreen("search") { SearchScreen(it) { _, _ -> } }
        catalogScreen("mixes", prepare = { it.start() }) { MixesScreen(it) { } }
        catalogScreen("downloads", prepare = { it.download(demoTracks.first()) }) { DownloadsScreen(it) { _, _ -> } }
        catalogScreen("lyrics") { LyricsScreen(demoTracks.first(), it) }
    }

    private fun catalogScreen(name: String, prepare: (CatalogViewModel) -> Unit = {}, content: @Composable (CatalogViewModel) -> Unit) {
        val viewModel = CatalogViewModel(DemoCatalogApi(), CoroutineScope(SupervisorJob() + Dispatchers.Unconfined))
        prepare(viewModel)
        renderScreen(name) { content(viewModel) }
        viewModel.close()
    }

    private fun renderScreen(name: String, content: @Composable () -> Unit) {
        ImageComposeScene(width = 390, height = 844) {
            MusicTheme {
                Surface(Modifier.fillMaxSize()) { content() }
            }
        }.use { scene ->
            scene.render()
            save(scene.render(), name)
        }
    }

    private fun save(image: Image, name: String): ByteArray {
        val bytes = image.encodeToData(EncodedImageFormat.PNG)?.bytes ?: error("Cannot encode $name")
        val output = File("build/screenshots/$name.png")
        output.parentFile.mkdirs()
        output.writeBytes(bytes)
        check(output.length() > 1_000) { "$name screenshot is empty" }
        return bytes
    }

    private inline fun withDemo(block: () -> Unit) {
        val previous = System.getProperty("pm.compose.demo")
        try {
            System.setProperty("pm.compose.demo", "1")
            block()
        } finally {
            if (previous == null) System.clearProperty("pm.compose.demo")
            else System.setProperty("pm.compose.demo", previous)
        }
    }
}
