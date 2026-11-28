package com.dec.privatemusic.ui

import androidx.compose.foundation.background
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
import androidx.compose.material3.Button
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
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.dec.privatemusic.player.PlayerState
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.SeekGate
import com.dec.privatemusic.player.Track

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
    viewModel: PlayerViewModel,
    onClose: () -> Unit,
    onQueue: () -> Unit,
    onLyrics: () -> Unit,
) {
    val error by viewModel.error.collectAsState()
    BoxWithConstraints(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(SelectedPanel, AppBackground)))) {
        val sideBySide = maxWidth >= 740.dp
        val coverSize = minOf(maxWidth - 40.dp, maxHeight * (if (sideBySide) 0.56f else 0.39f), 352.dp)
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
                GlyphButton(MusicIcon.Queue, "Очередь", onQueue)
            }
            Spacer(Modifier.height(if (sideBySide) 40.dp else 14.dp))
            val cover: @Composable () -> Unit = {
                CoverArt(Modifier.size(coverSize), track?.artwork_url, track?.key.orEmpty(), radius = 24)
            }
            if (sideBySide) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(40.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    cover()
                    PlayerDetails(Modifier.weight(1f), state, viewModel, onLyrics, error)
                }
            } else {
                cover()
                Spacer(Modifier.height(20.dp))
                PlayerDetails(Modifier.fillMaxWidth(), state, viewModel, onLyrics, error)
            }
        }
    }
}

@Composable
private fun PlayerDetails(
    modifier: Modifier,
    state: PlayerState,
    viewModel: PlayerViewModel,
    onLyrics: () -> Unit,
    error: String?,
) {
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            state.track?.title ?: "Ничего не играет",
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.Bold,
            textAlign = TextAlign.Center,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        state.track?.let {
            Text(it.artist, Modifier.padding(top = 4.dp), color = TextMuted, style = MaterialTheme.typography.titleMedium,
                maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.height(12.dp))
        PositionControl(state, viewModel::seek)
        Row(
            Modifier.padding(top = 14.dp),
            horizontalArrangement = Arrangement.spacedBy(24.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphButton(MusicIcon.Previous, "Предыдущий трек", viewModel::previous, enabled = state.track != null)
            PlayButton(state, viewModel::toggle)
            GlyphButton(MusicIcon.Next, "Следующий трек", viewModel::next, enabled = state.can_next)
        }
        TextButton(onClick = onLyrics, enabled = state.track != null) { Text("Текст песни") }
        if (error != null) ErrorNotice(error)
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
internal fun QueueScreen(tracks: List<Track>, current: Int?, onBack: () -> Unit, onJump: (Int) -> Unit) {
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                GlyphButton(MusicIcon.Back, "Назад", onBack)
                PageTitle("Очередь")
            }
        }
        if (tracks.isEmpty()) {
            item { EmptyState(MusicIcon.Queue, "Очередь пуста", "Воспроизведи трек или микс, чтобы заполнить очередь.") }
        }
        itemsIndexed(tracks) { index, track -> TrackRow(track, active = index == current, onClick = { onJump(index) }) }
    }
}
