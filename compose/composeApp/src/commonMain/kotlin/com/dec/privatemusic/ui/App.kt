package com.dec.privatemusic.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.dec.privatemusic.player.*
import kotlinx.coroutines.launch

private enum class Screen(val title: String, val icon: MusicIcon) {
    Home("Главная", MusicIcon.Home),
    Search("Поиск", MusicIcon.Search),
    Mixes("Миксы", MusicIcon.Mix),
    Downloads("Загрузки", MusicIcon.Download),
    Profile("Профиль", MusicIcon.Profile),
    Player("Плеер", MusicIcon.Note),
    Queue("Очередь", MusicIcon.Queue),
    Lyrics("Текст песни", MusicIcon.Lyrics),
    Equalizer("Эквалайзер", MusicIcon.Equalizer),
}

private val MainScreens = listOf(Screen.Home, Screen.Search, Screen.Mixes, Screen.Downloads, Screen.Profile)

@Composable
fun PrivateMusicApp(
    viewModel: PlayerViewModel,
    sessionViewModel: SessionViewModel,
    onLogin: (() -> Unit)?,
    onLogout: () -> Unit,
    bindBackHandler: @Composable (Boolean, () -> Unit) -> Unit = { _, _ -> },
) {
    val state by viewModel.state.collectAsState()
    val queue by viewModel.queue.collectAsState()
    val playerError by viewModel.error.collectAsState()
    val session by sessionViewModel.state.collectAsState()
    val catalog = remember(session.signed_in) { CatalogViewModel(platformCatalogApi()) }
    val library by catalog.library.collectAsState()
    val recommendations by catalog.recommendations.collectAsState()
    val catalogError by catalog.error.collectAsState()
    val tracks = when {
        catalog.isDemo -> library.ifEmpty { recommendations.ifEmpty { demoTracks } }
        session.signed_in -> library.ifEmpty { recommendations }
        else -> emptyList()
    }
    var screen by remember { mutableStateOf(Screen.Home) }
    var history by remember { mutableStateOf<List<Screen>>(emptyList()) }

    LaunchedEffect(viewModel) { viewModel.start(sessionViewModel::refresh) }
    LaunchedEffect(sessionViewModel) { sessionViewModel.start() }
    LaunchedEffect(session.signed_in) { if (session.signed_in) catalog.start() }
    LaunchedEffect(session.signed_in, screen) {
        if (!session.signed_in && !catalog.isDemo && (screen == Screen.Search || screen == Screen.Mixes)) {
            screen = Screen.Profile
        }
    }
    DisposableEffect(viewModel) { onDispose { viewModel.close() } }
    DisposableEffect(sessionViewModel) { onDispose { sessionViewModel.close() } }
    DisposableEffect(catalog) { onDispose { catalog.close() } }

    fun available(destination: Screen): Screen = when {
        (destination == Screen.Search || destination == Screen.Mixes) && !session.signed_in && !catalog.isDemo -> Screen.Profile
        else -> destination
    }

    fun navigate(destination: Screen) {
        history = emptyList()
        screen = available(destination)
    }

    fun open(destination: Screen) {
        if (screen == destination) return
        val index = history.indexOfLast { it == destination }
        history = if (index >= 0) history.take(index) else history + screen
        screen = destination
    }

    fun openPlayer() = open(Screen.Player)
    fun openQueue() = open(Screen.Queue)

    fun goBack() {
        screen = available(history.lastOrNull() ?: Screen.Home)
        history = history.dropLast(1)
    }

    bindBackHandler(screen != Screen.Home, ::goBack)

    MusicTheme {
        BoxWithConstraints(Modifier.fillMaxSize().background(AppBackground)) {
            val wide = maxWidth >= 820.dp
            Row(Modifier.fillMaxSize()) {
                if (wide) DesktopNavigation(screen, ::navigate)
                Scaffold(
                    modifier = Modifier.weight(1f),
                    containerColor = AppBackground,
                    bottomBar = {
                        Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
                            if (state.track != null && screen != Screen.Player) {
                                MiniPlayer(
                                    state = state,
                                    roomForControls = wide,
                                    onOpen = ::openPlayer,
                                    onToggle = viewModel::toggle,
                                    onNext = viewModel::next,
                                    onPrevious = viewModel::previous,
                                    onQueue = ::openQueue,
                                    onVolume = viewModel::setVolume,
                                )
                            }
                            if (!wide && screen != Screen.Player) MobileNavigation(screen, ::navigate)
                        }
                    },
                ) { padding ->
                    Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
                        Box(Modifier.widthIn(max = 1180.dp).fillMaxSize()) {
                            when (screen) {
                                Screen.Home -> HomeScreen(
                                    state = state,
                                    signedIn = session.signed_in || catalog.isDemo,
                                    tracks = tracks,
                                    libraryShown = library.isNotEmpty(),
                                    error = playerError ?: catalogError,
                                    onPlay = { viewModel.play(tracks, it) },
                                    onDownload = catalog::download,
                                    onPlayNext = viewModel::playNext,
                                    onPlayLast = viewModel::playLast,
                                    onOpenPlayer = ::openPlayer,
                                    onOpenQueue = ::openQueue,
                                    onOpenProfile = { navigate(Screen.Profile) },
                                )
                                Screen.Player -> FullPlayer(
                                    state = state,
                                    queue = queue,
                                    viewModel = viewModel,
                                    catalog = catalog,
                                    signedIn = session.signed_in || catalog.isDemo,
                                    onClose = ::goBack,
                                    onQueue = ::openQueue,
                                    onLyrics = { open(Screen.Lyrics) },
                                    onEqualizer = { open(Screen.Equalizer) },
                                )
                                Screen.Queue -> QueueScreen(queue, ::goBack, viewModel)
                                Screen.Equalizer -> EqualizerScreen(viewModel, ::goBack)
                                Screen.Profile -> ProfileScreen(session, onLogin, onLogout, sessionViewModel::refresh)
                                Screen.Search -> SearchScreen(catalog, viewModel::playNext, viewModel::playLast, viewModel::play)
                                Screen.Mixes -> MixesScreen(catalog) { mix -> viewModel.playRadio(mix.source, mix.tracks, 0, mix.nextOffset) }
                                Screen.Downloads -> DownloadsScreen(catalog, viewModel::playNext, viewModel::playLast, viewModel::play)
                                Screen.Lyrics -> LyricsScreen(state.track, catalog, state.position, ::goBack, viewModel::seek)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DesktopNavigation(screen: Screen, navigate: (Screen) -> Unit) {
    Column(
        Modifier.width(210.dp).fillMaxHeight().background(Panel).statusBarsPadding().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(top = 12.dp, bottom = 26.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(38.dp).clip(RoundedCornerShape(12.dp)).background(Highlight),
                contentAlignment = Alignment.Center,
            ) {
                Glyph(MusicIcon.Note, Modifier.size(22.dp), MaterialTheme.colorScheme.onPrimary)
            }
            Text("Private Music", color = TextStrong, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        }
        for (destination in MainScreens) {
            val active = screen == destination
            Row(
                Modifier.fillMaxWidth().heightIn(min = 50.dp).clip(RoundedCornerShape(14.dp))
                    .background(if (active) SelectedPanel else Panel)
                    .selectable(active, role = Role.Tab) { navigate(destination) }.padding(horizontal = 14.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val color = if (active) Highlight else TextMuted
                Glyph(destination.icon, Modifier.size(20.dp), color)
                Text(destination.title, color = color, style = MaterialTheme.typography.labelLarge, maxLines = 1)
            }
        }
    }
}

@Composable
private fun MobileNavigation(screen: Screen, navigate: (Screen) -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(Panel).padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (destination in MainScreens) {
            val active = screen == destination
            val color = if (active) Highlight else TextMuted
            Column(
                Modifier.weight(1f).heightIn(min = 58.dp).clip(RoundedCornerShape(12.dp))
                    .selectable(active, role = Role.Tab) { navigate(destination) }.padding(vertical = 5.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(3.dp, Alignment.CenterVertically),
            ) {
                Box(
                    Modifier.width(39.dp).height(26.dp).clip(RoundedCornerShape(9.dp))
                        .background(if (active) SelectedPanel else Panel),
                    contentAlignment = Alignment.Center,
                ) {
                    Glyph(destination.icon, Modifier.size(18.dp), color)
                }
                Text(destination.title, color = color, fontSize = 10.sp, maxLines = 1, softWrap = false)
            }
        }
    }
}

@Composable
private fun HomeScreen(
    state: PlayerState,
    signedIn: Boolean,
    tracks: List<Track>,
    libraryShown: Boolean,
    error: String?,
    onPlay: (Int) -> Unit,
    onDownload: (Track) -> Unit,
    onPlayNext: (Track) -> Unit,
    onPlayLast: (Track) -> Unit,
    onOpenPlayer: () -> Unit,
    onOpenQueue: () -> Unit,
    onOpenProfile: () -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val roomy = maxWidth >= 620.dp
        val gutter = if (roomy) 34.dp else 20.dp
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = gutter, end = gutter, top = if (roomy) 36.dp else 22.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(9.dp),
        ) {
            item {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    PageTitle("Главная", Modifier.weight(1f))
                    GlyphButton(MusicIcon.Queue, "Очередь", onOpenQueue)
                }
            }
            state.track?.let { track -> item { PlayingFeature(track, roomy, onOpenPlayer) } }
            if (!signedIn) {
                item {
                    Column(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp))
                            .background(Brush.horizontalGradient(listOf(SelectedPanel, Panel))).padding(24.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Glyph(MusicIcon.Note, Modifier.size(31.dp), Highlight)
                        Text("Твоя музыка в VK", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
                        Text("Войди, чтобы открыть медиатеку, поиск и персональные миксы.", color = TextMuted)
                        Button(onClick = onOpenProfile) { Text("Подключить VK") }
                    }
                }
            }
            if (error != null) item { ErrorNotice(error) }
            if (tracks.isNotEmpty()) {
                item {
                    Text(
                        if (libraryShown) "Моя музыка" else "Рекомендации",
                        Modifier.padding(top = 14.dp, bottom = 4.dp),
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
                itemsIndexed(tracks) { index, track ->
                    TrackRow(track, state.track?.key == track.key, { onPlay(index) }) {
                        TrackActions(track, onPlayNext, onPlayLast, "Скачать трек", { onDownload(track) })
                    }
                }
            } else if (signedIn && error == null) {
                item { EmptyState(MusicIcon.Note, "Пока нет треков", "Открой поиск или выбери микс, чтобы начать слушать.") }
            }
        }
    }
}

@Composable
private fun PlayingFeature(track: Track, roomy: Boolean, onOpen: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp))
            .background(Brush.horizontalGradient(listOf(SelectedPanel, Panel))).clickable(onClick = onOpen)
            .padding(if (roomy) 24.dp else 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(15.dp),
    ) {
        val coverSize = if (roomy) 106.dp else 68.dp
        CoverArt(Modifier.size(coverSize), track.artwork_url, track.key)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Сейчас играет", color = Highlight, style = MaterialTheme.typography.labelMedium)
            Text(track.title, style = if (roomy) MaterialTheme.typography.headlineSmall else MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(track.artist, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Glyph(MusicIcon.Expand, Modifier.size(20.dp), Highlight)
    }
}

@Composable
internal fun TrackRow(
    track: Track,
    active: Boolean,
    onClick: () -> Unit,
    endAction: (@Composable () -> Unit)? = null,
) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp).clip(RoundedCornerShape(13.dp))
            .background(if (active) SelectedPanel else Panel).clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CoverArt(Modifier.size(43.dp), track.artwork_url, track.key, radius = 9)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(track.title, color = if (active) Highlight else TextStrong,
                style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(track.artist, color = TextMuted,
                style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Text(formatTime(track.duration), color = TextMuted, style = MaterialTheme.typography.bodySmall)
        endAction?.invoke()
    }
}

internal fun formatTime(seconds: Int): String {
    val time = seconds.coerceAtLeast(0)
    return "${time / 60}:${(time % 60).toString().padStart(2, '0')}"
}
