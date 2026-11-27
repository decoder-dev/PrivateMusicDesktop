package com.dec.privatemusic.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.toComposeImageBitmap
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

@OptIn(ExperimentalComposeUiApi::class)
class ScreenshotTest {
    @Test
    fun homeAndFullPlayerRenderToPng() = runBlocking {
        System.setProperty("pm.compose.demo", "1")
        val api = DemoPlayerApi()
        api.playTracks(demoTracks, 1)
        val viewModel = PlayerViewModel(api)
        val session = SessionViewModel()
        ImageComposeScene(width = 360, height = 800) {
            PrivateMusicApp(
                viewModel = viewModel,
                sessionViewModel = session,
                onLogin = null,
                onLogout = { session.refresh() },
            )
        }.use { scene ->
            scene.render()
            save(scene.render(), "home")
            scene.sendPointerEvent(PointerEventType.Press, Offset(180f, 750f), type = PointerType.Touch)
            scene.sendPointerEvent(PointerEventType.Release, Offset(180f, 750f), type = PointerType.Touch)
            scene.render()
            save(scene.render(), "player")
        }
        viewModel.close()
    }

    @Test
    fun catalogScreensRenderToPng() {
        System.setProperty("pm.compose.demo", "1")
        renderScreen("profile") {
            ProfileScreen(SessionState(signed_in = true, profile = Profile(first_name = "Demo", last_name = "Listener")), null, {}, {})
        }
        catalogScreen("search") { SearchScreen(it) { _, _ -> } }
        catalogScreen("mixes") { MixesScreen(it) { } }
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
        ImageComposeScene(width = 360, height = 800) {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface(Modifier.fillMaxSize()) { content() }
            }
        }.use { scene ->
            scene.render()
            save(scene.render(), name)
        }
    }

    private fun save(image: Image, name: String) {
        val bytes = image.encodeToData(EncodedImageFormat.PNG)?.bytes ?: error("Cannot encode $name")
        val output = File("build/screenshots/$name.png")
        output.parentFile.mkdirs()
        output.writeBytes(bytes)
        check(output.length() > 1_000) { "$name screenshot is empty" }
        image.toComposeImageBitmap()
    }
}
