package com.dec.privatemusic.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.dec.privatemusic.player.*
import kotlinx.coroutines.launch

private val Background = Color(0xFF101012)
private val Surface = Color(0xFF1B1B1F)
private val SurfaceRaised = Color(0xFF25252B)
private val Accent = Color(0xFFE3C66B)
private val TextPrimary = Color(0xFFF5F3F7)
private val TextSecondary = Color(0xFFADA7B1)

private enum class Screen { Home, Player, Queue, Profile, Search, Mixes, Downloads, Lyrics }

@Composable
fun PrivateMusicApp(viewModel: PlayerViewModel, sessionViewModel: SessionViewModel, onLogin: (() -> Unit)?, onLogout: () -> Unit) {
    val state by viewModel.state.collectAsState()
    val queue by viewModel.queue.collectAsState()
    val error by viewModel.error.collectAsState()
    val session by sessionViewModel.state.collectAsState()
    val catalog = remember { CatalogViewModel(platformCatalogApi()) }
    val library by catalog.library.collectAsState()
    val recommendations by catalog.searchResults.collectAsState()
    val homeTracks = library.ifEmpty { recommendations.ifEmpty { demoTracks } }
    var screen by remember { mutableStateOf(Screen.Home) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(viewModel) { viewModel.start() }
    LaunchedEffect(sessionViewModel) { sessionViewModel.start() }
    LaunchedEffect(session.signed_in) { if (session.signed_in) catalog.start() }
    DisposableEffect(viewModel) { onDispose { viewModel.close() } }
    DisposableEffect(sessionViewModel) { onDispose { sessionViewModel.close() } }
    DisposableEffect(catalog) { onDispose { catalog.close() } }

    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Accent,
            background = Background,
            surface = Surface,
            surfaceVariant = SurfaceRaised,
            onPrimary = Background,
            onBackground = TextPrimary,
            onSurface = TextPrimary,
            onSurfaceVariant = TextSecondary,
        ),
    ) {
        Scaffold(
            containerColor = Background,
            bottomBar = {
                if (screen != Screen.Player && state.track != null) {
                    MiniPlayer(
                        state = state,
                        onOpen = { screen = Screen.Player },
                        onToggle = { scope.launch { viewModel.toggle() } },
                        onNext = { scope.launch { viewModel.next() } },
                    )
                }
            },
        ) { padding ->
            Box(Modifier.fillMaxSize().padding(padding)) {
                when (screen) {
                    Screen.Home -> HomeScreen(
                        state = state,
                        tracks = homeTracks,
                        error = error,
                        onPlay = { index -> scope.launch { viewModel.play(homeTracks, index) } },
                        onDownload = { track -> catalog.download(track) },
                        onOpenPlayer = { screen = Screen.Player },
                        onOpenQueue = { screen = Screen.Queue },
                        onOpenProfile = { screen = Screen.Profile },
                        onOpenSearch = { screen = Screen.Search },
                        onOpenMixes = { screen = Screen.Mixes },
                        onOpenDownloads = { screen = Screen.Downloads },
                    )
                    Screen.Player -> FullPlayer(
                        state = state,
                        viewModel = viewModel,
                        onClose = { screen = Screen.Home },
                        onQueue = { screen = Screen.Queue },
                        onLyrics = { screen = Screen.Lyrics },
                    )
                    Screen.Queue -> QueueScreen(
                        tracks = queue.ifEmpty { demoTracks },
                        current = state.queue_index,
                        onBack = { screen = if (state.track != null) Screen.Player else Screen.Home },
                        onJump = { index -> scope.launch { viewModel.jump(index) } },
                    )
                    Screen.Profile -> ProfileScreen(session, onLogin, onLogout) { sessionViewModel.refresh() }
                    Screen.Search -> SearchScreen(catalog) { tracks, index -> scope.launch { viewModel.play(tracks, index) } }
                    Screen.Mixes -> MixesScreen(catalog) { launch -> scope.launch { viewModel.playRadio(launch.source, launch.tracks, 0, launch.nextOffset) } }
                    Screen.Downloads -> DownloadsScreen(catalog) { tracks, index -> scope.launch { viewModel.play(tracks, index) } }
                    Screen.Lyrics -> LyricsScreen(state.track, catalog)
                }
                if (screen !in setOf(Screen.Home, Screen.Player, Screen.Queue)) {
                    FloatingActionButton(
                        onClick = { screen = if (screen == Screen.Lyrics) Screen.Player else Screen.Home },
                        modifier = Modifier.align(Alignment.BottomEnd).padding(20.dp).semantics { contentDescription = "Назад" },
                    ) { Text("‹", fontSize = 28.sp) }
                }
            }
        }
    }
}

@Composable
private fun HomeScreen(
    state: PlayerState,
    tracks: List<Track>,
    error: String?,
    onPlay: (Int) -> Unit,
    onDownload: (Track) -> Unit,
    onOpenPlayer: () -> Unit,
    onOpenQueue: () -> Unit,
    onOpenProfile: () -> Unit,
    onOpenSearch: () -> Unit,
    onOpenMixes: () -> Unit,
    onOpenDownloads: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 20.dp, top = 28.dp, end = 20.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Text("Главная", color = TextPrimary, fontSize = 30.sp, fontWeight = FontWeight.Bold)
        }
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TextButton(onClick = onOpenProfile) { Text("Профиль") }
                TextButton(onClick = onOpenSearch) { Text("Поиск") }
                TextButton(onClick = onOpenMixes) { Text("Миксы") }
                TextButton(onClick = onOpenDownloads) { Text("Загрузки") }
            }
        }
        item {
            Card(
                modifier = Modifier.fillMaxWidth().clickable(onClick = onOpenPlayer),
                colors = CardDefaults.cardColors(containerColor = SurfaceRaised),
                shape = RoundedCornerShape(24.dp),
            ) {
                Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
                    CoverArt(Modifier.size(64.dp))
                    Spacer(Modifier.width(16.dp))
                    Column(Modifier.weight(1f)) {
                        Text(if (state.track != null) "СЕЙЧАС ИГРАЕТ" else "ВАША МУЗЫКА", color = Accent, fontSize = 11.sp)
                        Text(state.track?.artist ?: "Private Music", color = TextPrimary, fontSize = 21.sp, fontWeight = FontWeight.SemiBold)
                        Text(state.track?.title ?: "Выберите трек из списка", color = TextSecondary, fontSize = 14.sp)
                    }
                }
            }
        }
        if (error != null) item { Text(error, color = MaterialTheme.colorScheme.error) }
        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Рекомендации", color = TextPrimary, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                TextButton(onClick = onOpenQueue) { Text("Очередь") }
            }
        }
        itemsIndexed(tracks, key = { _, track -> track.key }) { index, track ->
            TrackRow(track = track, active = state.track?.key == track.key, onClick = { onPlay(index) }, onDownload = { onDownload(track) })
        }
    }
}

@Composable
internal fun TrackRow(track: Track, active: Boolean, onClick: () -> Unit, onDownload: (() -> Unit)? = null) {
    Card(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick),
        colors = CardDefaults.cardColors(containerColor = if (active) SurfaceRaised else Color.Transparent),
        shape = RoundedCornerShape(16.dp),
    ) {
        Row(Modifier.padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            CoverArt(Modifier.size(46.dp))
            Column(Modifier.padding(start = 12.dp).weight(1f)) {
                Text(track.title, color = if (active) Accent else TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(track.artist, color = TextSecondary, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Text(formatTime(track.duration), color = TextSecondary, fontSize = 13.sp)
            if (onDownload != null) {
                IconButton(onClick = onDownload, modifier = Modifier.semantics { contentDescription = "Скачать ${track.title}" }) {
                    Text("↓", color = TextPrimary, fontSize = 22.sp)
                }
            }
        }
    }
}

@Composable
private fun MiniPlayer(state: PlayerState, onOpen: () -> Unit, onToggle: () -> Unit, onNext: () -> Unit) {
    var drag = 0f
    Card(
        modifier = Modifier
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .fillMaxWidth()
            .pointerInput(Unit) {
                detectVerticalDragGestures(
                    onDragEnd = { if (drag < -40f) onOpen() },
                    onVerticalDrag = { _, amount -> drag += amount },
                )
            }
            .clickable(onClick = onOpen),
        colors = CardDefaults.cardColors(containerColor = SurfaceRaised),
        shape = RoundedCornerShape(18.dp),
    ) {
        Column {
            Row(Modifier.padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                CoverArt(Modifier.size(44.dp))
                Column(Modifier.padding(start = 10.dp).weight(1f)) {
                    Text(state.track?.title.orEmpty(), color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(state.track?.artist.orEmpty(), color = TextSecondary, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                TransportButton(if (state.status == "playing") "Ⅱ" else "▶", "Пауза / играть", onToggle)
                TransportButton("⏭", "Следующий трек", onNext)
            }
            LinearProgressIndicator(
                progress = { if (state.duration > 0) (state.position / state.duration).toFloat().coerceIn(0f, 1f) else 0f },
                modifier = Modifier.fillMaxWidth().height(2.dp),
                color = Accent,
                trackColor = Color.Transparent,
            )
        }
    }
}

@Composable
private fun FullPlayer(state: PlayerState, viewModel: PlayerViewModel, onClose: () -> Unit, onQueue: () -> Unit, onLyrics: () -> Unit) {
    val scope = rememberCoroutineScope()
    var seekVersion by remember { mutableStateOf(0) }
    val seekGate = remember { SeekGate(scope, onSettle = { seekVersion += 1 }) }
    var sliderValue by remember { mutableStateOf(0f) }
    seekGate.track(state.track?.key)
    val renderPosition = seekVersion >= 0 && seekGate.shouldRender(state.position)
    if (renderPosition) sliderValue = state.position.toFloat()

    BoxWithConstraints(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(Color(0xFF30271B), Background)))) {
        Column(
            modifier = Modifier.widthIn(max = 620.dp).fillMaxWidth().align(Alignment.TopCenter)
                .padding(start = 24.dp, top = 16.dp, end = 24.dp, bottom = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                TransportButton("⌄", "Закрыть плеер", onClose)
                Text("СЕЙЧАС ИГРАЕТ", color = TextSecondary, fontSize = 12.sp, modifier = Modifier.weight(1f))
                TransportButton("≡", "Очередь", onQueue)
            }
            Spacer(Modifier.height(24.dp))
            CoverArt(Modifier.size(240.dp))
            Spacer(Modifier.height(24.dp))
            Text(state.track?.title ?: "Ничего не играет", color = TextPrimary, fontSize = 26.sp, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(state.track?.artist.orEmpty(), color = TextSecondary, fontSize = 17.sp)
            Spacer(Modifier.height(24.dp))
            Slider(
                value = sliderValue,
                onValueChange = {
                    sliderValue = it
                    seekGate.preview()
                },
                onValueChangeFinished = {
                    val target = sliderValue.toDouble()
                    seekGate.commit(target)
                    scope.launch { viewModel.seek(target) }
                },
                valueRange = 0f..state.duration.toFloat().coerceAtLeast(1f),
                modifier = Modifier.fillMaxWidth(),
                colors = SliderDefaults.colors(thumbColor = TextPrimary, activeTrackColor = TextPrimary, inactiveTrackColor = SurfaceRaised),
            )
            Row(Modifier.fillMaxWidth()) {
                Text(formatTime(sliderValue.toInt()), color = TextSecondary, fontSize = 12.sp)
                Spacer(Modifier.weight(1f))
                Text("-${formatTime((state.duration.toInt() - sliderValue.toInt()).coerceAtLeast(0))}", color = TextSecondary, fontSize = 12.sp)
            }
            Spacer(Modifier.height(16.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(28.dp), verticalAlignment = Alignment.CenterVertically) {
                TransportButton("⏮", "Предыдущий трек") { scope.launch { viewModel.previous() } }
                Button(
                    onClick = { scope.launch { viewModel.toggle() } },
                    modifier = Modifier.size(76.dp),
                    shape = RoundedCornerShape(50),
                    colors = ButtonDefaults.buttonColors(containerColor = TextPrimary, contentColor = Background),
                    contentPadding = PaddingValues(0.dp),
                ) { Text(if (state.status == "playing") "Ⅱ" else "▶", fontSize = 28.sp) }
                TransportButton("⏭", "Следующий трек") { scope.launch { viewModel.next() } }
            }
            TextButton(onClick = onLyrics, enabled = state.track != null) { Text("Текст песни") }
            Spacer(Modifier.height(4.dp))
            Text("Потяните мини-плеер вверх, чтобы вернуться сюда", color = TextSecondary, fontSize = 12.sp)
        }
    }
}

@Composable
private fun QueueScreen(tracks: List<Track>, current: Int?, onBack: () -> Unit, onJump: (Int) -> Unit) {
    Column(Modifier.fillMaxSize().padding(20.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TransportButton("‹", "Назад", onBack)
            Text("Очередь", color = TextPrimary, fontSize = 28.sp, fontWeight = FontWeight.Bold)
        }
        LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp), contentPadding = PaddingValues(top = 20.dp)) {
            itemsIndexed(tracks, key = { _, track -> track.key }) { index, track ->
                TrackRow(track = track, active = index == current, onClick = { onJump(index) })
            }
        }
    }
}

@Composable
internal fun CoverArt(modifier: Modifier = Modifier) {
    Box(
        modifier = modifier.clip(RoundedCornerShape(16.dp))
            .background(Brush.linearGradient(listOf(Color(0xFFD9D3C3), Color(0xFF7F754E), Color(0xFF343129)))),
        contentAlignment = Alignment.Center,
    ) { Text("♪", color = TextPrimary, fontSize = 26.sp) }
}

@Composable
private fun TransportButton(glyph: String, description: String, onClick: () -> Unit) {
    IconButton(onClick = onClick, modifier = Modifier.semantics { contentDescription = description }) {
        Text(glyph, color = TextPrimary, fontSize = 24.sp)
    }
}

internal fun formatTime(seconds: Int): String = "%d:%02d".format(seconds.coerceAtLeast(0) / 60, seconds.coerceAtLeast(0) % 60)
