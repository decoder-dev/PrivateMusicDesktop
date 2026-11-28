package com.dec.privatemusic.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.dec.privatemusic.player.PlayerViewModel
import com.dec.privatemusic.player.SleepTimer
import kotlin.math.roundToInt

private val frequencies = listOf("31", "62", "125", "250", "500", "1 000", "2 000", "4 000", "8 000", "16 000")
private val flat = List(frequencies.size) { 0f }
private val presets = mapOf(
    "Ровно" to flat,
    "Больше баса" to listOf(6f, 6f, 5f, 3f, 1f, 0f, 0f, 0f, -1f, -1f),
    "Вокал" to listOf(-2f, -2f, -1f, 0f, 2f, 4f, 4f, 3f, 1f, 0f),
    "Рок" to listOf(4f, 3f, 2f, 0f, -1f, 1f, 3f, 4f, 4f, 3f),
    "Электронная" to listOf(5f, 4f, 2f, 0f, -1f, 0f, 2f, 3f, 5f, 5f),
    "Наушники" to listOf(3f, 5f, 3f, -1f, -1f, 0f, 2f, 4f, 5f, 4f),
)

@Composable
internal fun EqualizerScreen(viewModel: PlayerViewModel, onBack: () -> Unit) {
    val applied by viewModel.equalizer.collectAsState()
    val error by viewModel.error.collectAsState()
    var draft by remember { mutableStateOf(flat) }
    LaunchedEffect(applied) {
        applied?.let { draft = it }
    }
    var choosePreset by remember { mutableStateOf(false) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            Modifier.widthIn(max = 740.dp).fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                GlyphButton(MusicIcon.Back, "Назад к плееру", onBack)
                PageTitle("Эквалайзер", Modifier.weight(1f))
            }
            Row(
                Modifier.fillMaxWidth().background(Panel, RoundedCornerShape(14.dp)).padding(horizontal = 14.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Обработка звука", style = MaterialTheme.typography.titleMedium)
                Switch(
                    checked = applied != null,
                    onCheckedChange = { viewModel.setEqualizer(if (it) draft else null) },
                    modifier = Modifier.semantics { contentDescription = "Включить эквалайзер" },
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Box {
                    val activePreset = presets.keys.firstOrNull { presets[it] == draft }
                    OutlinedButton({ choosePreset = true }) { Text(activePreset ?: "Свой профиль") }
                    DropdownMenu(choosePreset, { choosePreset = false }) {
                        presets.forEach { (title, gains) ->
                            DropdownMenuItem({ Text(title) }, {
                                choosePreset = false
                                draft = gains
                                viewModel.setEqualizer(gains)
                            })
                        }
                    }
                }
                TextButton({ draft = flat; viewModel.setEqualizer(if (applied != null) flat else null) }) {
                    Text("Сбросить")
                }
            }
            if (error != null) ErrorNotice(error.orEmpty())
            BoxWithConstraints(Modifier.fillMaxWidth().background(Panel, RoundedCornerShape(14.dp)).padding(12.dp)) {
                val groups = if (maxWidth >= 580.dp) frequencies.indices.chunked(5) else listOf(frequencies.indices.toList())
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    groups.forEach { indices ->
                        Column(Modifier.weight(1f)) {
                            indices.forEach { index ->
                                Row(
                                    Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    Text("${frequencies[index]} Гц", Modifier.width(66.dp),
                                        color = TextMuted, style = MaterialTheme.typography.labelMedium)
                                    Slider(
                                        value = draft[index],
                                        onValueChange = { gain ->
                                            draft = draft.toMutableList().apply { this[index] = gain.roundToInt().toFloat() }
                                        },
                                        onValueChangeFinished = { viewModel.setEqualizer(draft) },
                                        enabled = applied != null,
                                        valueRange = -12f..12f,
                                        steps = 23,
                                        modifier = Modifier.weight(1f).semantics { contentDescription = "${frequencies[index]} Гц" },
                                    )
                                    Text("${draft[index].roundToInt()} дБ", Modifier.width(42.dp),
                                        style = MaterialTheme.typography.labelMedium)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun SleepTimerDialog(
    timer: SleepTimer,
    onDismiss: () -> Unit,
    onSet: (Long?, Boolean) -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Таймер сна") },
        text = {
            Column(Modifier.widthIn(max = 320.dp).verticalScroll(rememberScrollState())) {
                if (timer.active) {
                    val status = if (timer.end_of_track) "Остановка после текущего трека"
                    else "Осталось ${formatTime(timer.remaining?.toInt() ?: 0)}"
                    Text(status, color = Highlight, modifier = Modifier.padding(bottom = 8.dp))
                } else {
                    Text("Когда поставить музыку на паузу?", color = TextMuted, modifier = Modifier.padding(bottom = 8.dp))
                }
                listOf(15L, 30L, 45L, 60L).forEach { minutes ->
                    TextButton({ onSet(minutes * 60, false) }) { Text("Через $minutes мин") }
                }
                TextButton({ onSet(null, true) }) { Text("В конце трека") }
                if (timer.active) {
                    TextButton({ onSet(null, false) }) { Text("Выключить таймер") }
                }
            }
        },
        confirmButton = { TextButton(onDismiss) { Text("Закрыть") } },
    )
}
