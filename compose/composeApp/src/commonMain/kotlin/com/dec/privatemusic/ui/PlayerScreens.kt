package com.dec.privatemusic.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.progressSemantics
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.dec.privatemusic.player.CatalogViewModel
import com.dec.privatemusic.player.PlayerState
import com.dec.privatemusic.player.QueueEvent
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.SeekGate
import com.dec.privatemusic.player.Track
import kotlinx.coroutines.delay

private val PlayerShape = RoundedCornerShape(20.dp)

@Composable
internal fun MiniPlayer(
    state: PlayerState,
    roomForControls: Boolean,
    onOpen: () -> Unit,
    onToggle: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onQueue: () -> Unit,
    onVolume: (Float) -> Unit,
) {
    val track = state.track ?: return
    val open by rememberUpdatedState(onOpen)
    val progress = if (state.duration > 0.0) (state.position / state.duration).toFloat().coerceIn(0f, 1f) else 0f
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 5.dp)
            .clip(PlayerShape).background(RaisedPanel),
    ) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 9.dp, vertical = 5.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(
                Modifier.weight(1f).clip(RoundedCornerShape(12.dp))
                    .pointerInput(Unit) {
                        val threshold = 32.dp.toPx()
                        var dragged = 0f
                        detectVerticalDragGestures(
                            onDragStart = { dragged = 0f },
                            onDragEnd = { if (dragged < -threshold) open() },
                            onVerticalDrag = { _, amount -> dragged += amount },
                        )
                    }
                    .clickable(onClickLabel = "Открыть плеер", onClick = onOpen)
                    .padding(vertical = 3.dp, horizontal = 3.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                CoverArt(Modifier.size(42.dp), track.artwork_url, track.key, radius = 9)
                Column(Modifier.weight(1f)) {
                    Text(track.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(track.artist, color = TextMuted, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (roomForControls) GlyphButton(MusicIcon.Previous, "Предыдущий трек", onPrevious)
            PlayButton(state, onToggle, compact = true)
            GlyphButton(MusicIcon.Next, "Следующий трек", onNext, enabled = state.can_next)
            if (roomForControls) {
                GlyphButton(MusicIcon.Queue, "Очередь", onQueue)
                Row(Modifier.padding(end = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Glyph(MusicIcon.Volume, Modifier.size(18.dp), TextMuted)
                    Slider(
                        state.volume.coerceIn(0f, 1f),
                        onVolume,
                        Modifier.width(95.dp).semantics { contentDescription = "Громкость" },
                    )
                }
            }
        }
        Box(Modifier.fillMaxWidth().height(2.dp).progressSemantics(progress).background(Border)) {
            Box(Modifier.fillMaxHeight().fillMaxWidth(progress).background(Highlight))
        }
    }
}

@Composable
internal fun FullPlayer(
    state: PlayerState,
    queue: QueueEvent,
    viewModel: PlayerViewModel,
    catalog: CatalogViewModel,
    signedIn: Boolean,
    onClose: () -> Unit,
    onQueue: () -> Unit,
    onLyrics: () -> Unit,
    onEqualizer: () -> Unit,
) {
    val playbackError by viewModel.error.collectAsState()
    val catalogError by catalog.error.collectAsState()
    val timer by viewModel.sleepTimer.collectAsState()
    var visualizer by remember { mutableStateOf(false) }
    var timerOpen by remember { mutableStateOf(false) }
    val showSpectrum = visualizer && LocalWindowInfo.current.isWindowFocused
    DisposableEffect(viewModel, showSpectrum) {
        if (showSpectrum) viewModel.setVisualizer(true)
        onDispose { if (showSpectrum) viewModel.setVisualizer(false) }
    }
    LaunchedEffect(viewModel, timer.remaining != null) {
        while (timer.remaining != null) {
            delay(1_000)
            viewModel.refreshSleepTimer()
        }
    }
    if (timerOpen) {
        SleepTimerDialog(timer, { timerOpen = false }) { seconds, endOfTrack ->
            viewModel.setSleepTimer(seconds, endOfTrack)
            timerOpen = false
        }
    }
    val onTimer: () -> Unit = { timerOpen = true }
    val details: @Composable (Modifier) -> Unit = { modifier ->
        PlayerDetails(modifier, state, queue, viewModel, catalog, signedIn, onQueue, onLyrics,
            onEqualizer, onTimer, timer.active, visualizer, { visualizer = !visualizer }, playbackError ?: catalogError)
    }
    BoxWithConstraints(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(SelectedPanel, AppBackground)))) {
        val sideBySide = maxWidth >= 740.dp
        val coverSize = minOf(maxWidth - 40.dp, maxHeight * (if (sideBySide) 0.56f else 0.33f), 352.dp)
            .coerceAtLeast(100.dp)
        val track = state.track
        Column(
            Modifier.align(Alignment.TopCenter).widthIn(max = 980.dp).fillMaxWidth()
                .verticalScroll(rememberScrollState()).padding(horizontal = if (sideBySide) 36.dp else 20.dp, vertical = 18.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                GlyphButton(MusicIcon.Back, "Закрыть плеер", onClose)
                Text("Сейчас играет", Modifier.weight(1f), textAlign = TextAlign.Center,
                    color = TextMuted, style = MaterialTheme.typography.labelLarge)
                GlyphButton(MusicIcon.Timer, if (timer.active) "Таймер сна включён" else "Таймер сна",
                    onTimer, active = timer.active)
            }
            Spacer(Modifier.height(if (sideBySide) 40.dp else 14.dp))
            val cover: @Composable () -> Unit = {
                Box(Modifier.size(coverSize)) {
                    CoverArt(Modifier.fillMaxSize(), track?.artwork_url, track?.key.orEmpty(), radius = 24)
                    if (showSpectrum) {
                        val bands by viewModel.spectrum.collectAsState()
                        Spectrum(bands, Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(72.dp).padding(10.dp))
                    }
                }
            }
            if (sideBySide) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(40.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    cover()
                    details(Modifier.weight(1f))
                }
            } else {
                cover()
                Spacer(Modifier.height(20.dp))
                details(Modifier.fillMaxWidth())
            }
        }
    }
}

@Composable
private fun PlayerDetails(
    modifier: Modifier,
    state: PlayerState,
    queue: QueueEvent,
    viewModel: PlayerViewModel,
    catalog: CatalogViewModel,
    signedIn: Boolean,
    onQueue: () -> Unit,
    onLyrics: () -> Unit,
    onEqualizer: () -> Unit,
    onTimer: () -> Unit,
    timerActive: Boolean,
    visualizer: Boolean,
    onVisualizer: () -> Unit,
    error: String?,
) {
    val library by catalog.library.collectAsState()
    val favoriteCopies by catalog.favoriteCopies.collectAsState()
    val canFavorite by catalog.canFavorite.collectAsState()
    val favorite = state.track?.let { track -> library.firstOrNull { it.key == track.key } ?: favoriteCopies[track.key] }
    val track = state.track
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            if (signedIn) Spacer(Modifier.width(48.dp))
            Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(track?.title ?: "Ничего не играет",
                    style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold,
                    textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
                track?.let {
                    Text(it.artist, Modifier.padding(top = 4.dp), color = TextMuted,
                        style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center,
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (signedIn) GlyphButton(
                if (favorite == null) MusicIcon.Heart else MusicIcon.HeartFilled,
                if (favorite == null) "Добавить в мою музыку" else "Убрать из моей музыки",
                { track?.let(catalog::toggleFavorite) },
                enabled = track != null && canFavorite,
                active = favorite != null,
            )
        }
        if (state.status == "loading" || state.status == "buffering") {
            Text(if (state.status == "loading") "Загрузка трека" else "Буферизация",
                color = Highlight, style = MaterialTheme.typography.labelMedium)
        }
        Spacer(Modifier.height(12.dp))
        PositionControl(state, viewModel::seek)
        Row(
            Modifier.padding(top = 14.dp),
            horizontalArrangement = Arrangement.spacedBy(3.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphButton(MusicIcon.Shuffle, if (state.shuffle) "Выключить перемешивание" else "Перемешать",
                viewModel::toggleShuffle, enabled = track != null, active = state.shuffle)
            GlyphButton(MusicIcon.Previous, "Предыдущий трек", viewModel::previous, enabled = track != null)
            PlayButton(state, viewModel::toggle)
            GlyphButton(MusicIcon.Next, "Следующий трек", viewModel::next, enabled = state.can_next)
            val repeatLabel = when (state.repeat) {
                "one" -> "Повтор трека"
                "all" -> "Повтор очереди"
                else -> "Повтор выключен"
            }
            GlyphButton(if (state.repeat == "one") MusicIcon.RepeatOne else MusicIcon.Repeat,
                "Режим повтора. $repeatLabel", viewModel::cycleRepeat, enabled = track != null, active = state.repeat != "off")
        }
        Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            GlyphButton(if (state.volume == 0f) MusicIcon.Mute else MusicIcon.Volume,
                if (state.volume == 0f) "Включить звук" else "Выключить звук", viewModel::toggleMute)
            Slider(state.volume.coerceIn(0f, 1f), viewModel::setVolume,
                Modifier.weight(1f).semantics { contentDescription = "Громкость" })
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            PlayerShortcut("Текст", MusicIcon.Lyrics, onLyrics, Modifier.weight(1f), enabled = track != null)
            PlayerShortcut("Очередь", MusicIcon.Queue, onQueue, Modifier.weight(1f))
            PlayerShortcut("Эквалайзер", MusicIcon.Equalizer, onEqualizer, Modifier.weight(1.3f))
            Box(Modifier.weight(1f)) {
                var more by remember { mutableStateOf(false) }
                PlayerShortcut("Ещё", MusicIcon.More, { more = true }, Modifier.fillMaxWidth(), active = timerActive || visualizer)
                DropdownMenu(more, { more = false }) {
                    if (signedIn && track != null) {
                        DropdownMenuItem({ Text("Скачать трек") }, { more = false; catalog.download(track) })
                    }
                    DropdownMenuItem({ Text(if (timerActive) "Таймер сна включён" else "Таймер сна") }, {
                        more = false
                        onTimer()
                    })
                    DropdownMenuItem({ Text(if (visualizer) "Скрыть визуализатор" else "Визуализатор") }, {
                        more = false
                        onVisualizer()
                    })
                }
            }
        }
        val order = queue.playbackIndices
        val nextIndex = order.getOrNull(queue.currentPosition + 1)
            ?: order.firstOrNull()?.takeIf { state.repeat == "all" }
        val next = nextIndex?.let(queue.tracks::getOrNull)
        if (queue.current != null && next != null && state.repeat != "one") {
            Text("Далее · ${next.title}", Modifier.padding(top = 10.dp), color = TextMuted,
                style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (error != null) ErrorNotice(error)
    }
}

@Composable
private fun PlayerShortcut(
    label: String,
    icon: MusicIcon,
    onClick: () -> Unit,
    modifier: Modifier,
    enabled: Boolean = true,
    active: Boolean = false,
) {
    val color = when {
        !enabled -> TextMuted.copy(alpha = 0.45f)
        active -> Highlight
        else -> TextStrong
    }
    Column(
        modifier.heightIn(min = 54.dp).clip(RoundedCornerShape(12.dp))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 2.dp, vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
    ) {
        Glyph(icon, Modifier.size(20.dp), color)
        Text(label, color = color, textAlign = TextAlign.Center, style = MaterialTheme.typography.labelMedium)
    }
}

@Composable
private fun Spectrum(bands: List<Float>, modifier: Modifier) {
    if (bands.isEmpty()) return
    Canvas(modifier.clip(RoundedCornerShape(10.dp)).background(AppBackground.copy(alpha = 0.72f))) {
        val slot = size.width / bands.size
        bands.forEachIndexed { index, level ->
            val height = (size.height - 10.dp.toPx()) * level.coerceIn(0f, 1f)
            drawRect(Highlight, Offset(slot * index + slot * 0.15f, size.height - 5.dp.toPx() - height),
                Size(slot * 0.70f, height))
        }
    }
}

@Composable
private fun PositionControl(state: PlayerState, onSeek: (Double) -> Unit) {
    val scope = rememberCoroutineScope()
    var settle by remember { mutableIntStateOf(0) }
    val gate = remember { SeekGate(scope, onSettle = { settle += 1 }) }
    var requested by remember { mutableFloatStateOf(0f) }
    val duration = state.duration.toFloat().coerceAtLeast(1f)
    gate.track(state.track?.key)
    val position = if (settle >= 0 && gate.shouldRender(state.position)) state.position.toFloat() else requested
    Column(Modifier.fillMaxWidth()) {
        Slider(
            value = position.coerceIn(0f, duration),
            onValueChange = {
                gate.preview()
                requested = it
            },
            onValueChangeFinished = {
                val target = requested.toDouble()
                gate.commit(target)
                onSeek(target)
            },
            enabled = state.track != null && state.duration > 0.0,
            valueRange = 0f..duration,
            modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Позиция трека" },
            colors = SliderDefaults.colors(activeTrackColor = Highlight, inactiveTrackColor = Border, thumbColor = Highlight),
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(formatTime(position.toInt()), color = TextMuted, style = MaterialTheme.typography.labelMedium)
            Text(formatTime(state.duration.toInt()), color = TextMuted, style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
private fun PlayButton(state: PlayerState, onClick: () -> Unit, compact: Boolean = false) {
    val playing = state.status == "playing"
    val label = if (playing) "Пауза" else "Воспроизвести"
    Button(
        onClick,
        Modifier.size(if (compact) 44.dp else 66.dp).semantics { contentDescription = label },
        enabled = state.track != null,
        shape = CircleShape,
        contentPadding = PaddingValues(0.dp),
        colors = ButtonDefaults.buttonColors(containerColor = Highlight, contentColor = MaterialTheme.colorScheme.onPrimary),
    ) {
        Glyph(if (playing) MusicIcon.Pause else MusicIcon.Play, Modifier.size(if (compact) 19.dp else 29.dp),
            MaterialTheme.colorScheme.onPrimary)
    }
}

@Composable
internal fun TrackActions(
    track: Track,
    onPlayNext: ((Track) -> Unit)?,
    onPlayLast: ((Track) -> Unit)?,
    extraLabel: String,
    onExtra: () -> Unit,
) {
    var expanded by remember(track.key) { mutableStateOf(false) }
    Box {
        GlyphButton(MusicIcon.More, "Действия с треком ${track.title}", { expanded = true })
        DropdownMenu(expanded, { expanded = false }) {
            if (onPlayNext != null) {
                DropdownMenuItem({ Text("Воспроизвести следующим") }, { expanded = false; onPlayNext(track) })
            }
            if (onPlayLast != null) {
                DropdownMenuItem({ Text("В конец очереди") }, { expanded = false; onPlayLast(track) })
            }
            DropdownMenuItem({ Text(extraLabel) }, { expanded = false; onExtra() })
        }
    }
}

@Composable
internal fun QueueScreen(queue: QueueEvent, onBack: () -> Unit, viewModel: PlayerViewModel) {
    val (tracks, current) = queue
    val playbackOrder = queue.playbackIndices
    val firstUpcoming = queue.currentPosition + 1
    val error by viewModel.error.collectAsState()
    var confirmClear by remember { mutableStateOf(false) }
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text("Очистить следующие треки?") },
            text = { Text("Текущий трек продолжит играть. Если включён микс, очередь перестанет пополняться.") },
            confirmButton = { TextButton({ confirmClear = false; viewModel.clearUpcoming() }) { Text("Очистить") } },
            dismissButton = { TextButton({ confirmClear = false }) { Text("Отмена") } },
        )
    }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                GlyphButton(MusicIcon.Back, "Назад", onBack)
                PageTitle("Очередь", Modifier.weight(1f))
            }
            if (tracks.size > firstUpcoming) {
                TextButton({ confirmClear = true }) { Text("Очистить следующие") }
            }
            if (error != null) ErrorNotice(error.orEmpty())
        }
        if (tracks.isEmpty()) {
            item { EmptyState(MusicIcon.Queue, "Очередь пуста", "Воспроизведи трек или микс, чтобы заполнить очередь.") }
        }
        itemsIndexed(playbackOrder) { position, index ->
            val track = tracks[index]
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                val section = when {
                    index == current -> "Сейчас играет"
                    position == firstUpcoming -> "Далее"
                    position == 0 -> "Недавно играло"
                    else -> null
                }
                if (section != null) Text(section, color = TextMuted, style = MaterialTheme.typography.labelLarge)
                TrackRow(track, active = index == current, onClick = { viewModel.jump(index) }) {
                    QueueTrackActions(
                        track.title,
                        onMoveUp = if (position > firstUpcoming) {
                            { viewModel.moveUpcoming(index, playbackOrder[position - 1]) }
                        } else null,
                        onMoveDown = if (position >= firstUpcoming && position < playbackOrder.lastIndex) {
                            { viewModel.moveUpcoming(index, playbackOrder[position + 1]) }
                        } else null,
                        onRemove = { viewModel.removeFromQueue(index) },
                    )
                }
            }
        }
    }
}

@Composable
private fun QueueTrackActions(title: String, onMoveUp: (() -> Unit)?, onMoveDown: (() -> Unit)?, onRemove: () -> Unit) {
    var actions by remember(title) { mutableStateOf(false) }
    Box {
        GlyphButton(MusicIcon.More, "Действия с треком $title", { actions = true })
        DropdownMenu(actions, { actions = false }) {
            if (onMoveUp != null) DropdownMenuItem({ Text("Выше в очереди") }, { actions = false; onMoveUp() })
            if (onMoveDown != null) DropdownMenuItem({ Text("Ниже в очереди") }, { actions = false; onMoveDown() })
            DropdownMenuItem({ Text("Убрать из очереди") }, { actions = false; onRemove() })
        }
    }
}
