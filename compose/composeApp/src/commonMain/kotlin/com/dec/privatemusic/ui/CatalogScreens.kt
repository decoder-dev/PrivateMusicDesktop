package com.dec.privatemusic.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.dec.privatemusic.player.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun ProfileScreen(session: SessionState, onLogin: (() -> Unit)?, onLogout: () -> Unit, onSessionChanged: () -> Unit) {
    var manualLogin by remember { mutableStateOf(false) }
    CatalogColumn("Профиль") {
        item {
            Column(
                Modifier.widthIn(max = 620.dp).fillMaxWidth().clip(RoundedCornerShape(24.dp))
                    .background(Panel).padding(24.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Box(
                    Modifier.size(58.dp).clip(RoundedCornerShape(19.dp)).background(SelectedPanel),
                    contentAlignment = Alignment.Center,
                ) {
                    Glyph(if (session.signed_in) MusicIcon.Check else MusicIcon.Profile, Modifier.size(28.dp), Highlight)
                }
                val name = listOf(session.profile?.first_name, session.profile?.last_name)
                    .filterNotNull().filter { it.isNotBlank() }.joinToString(" ")
                val title = when {
                    !session.signed_in -> "Требуется вход"
                    name.isNotEmpty() -> name
                    else -> "Аккаунт VK подключён"
                }
                Text(title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
                if (session.signed_in) {
                    if (name.isNotEmpty()) Text("Аккаунт VK подключён", color = TextMuted)
                    OutlinedButton(onClick = onLogout) { Text("Выйти") }
                } else {
                    Text("Войди в VK, чтобы слушать медиатеку и персональные миксы.", color = TextMuted)
                    Button(onClick = { if (onLogin != null) onLogin() else manualLogin = true }) { Text("Войти через VK") }
                }
            }
        }
    }
    if (manualLogin) SessionLoginDialog(onDismiss = { manualLogin = false }, onSessionChanged = onSessionChanged)
}

@Composable
private fun SessionLoginDialog(onDismiss: () -> Unit, onSessionChanged: () -> Unit) {
    var token by remember { mutableStateOf("") }
    var cookie by remember { mutableStateOf("") }
    var userId by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Сессия VK") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("На компьютере можно подключить существующую сессию VK.", color = TextMuted)
                OutlinedTextField(
                    token, { token = it }, label = { Text("Токен доступа") },
                    singleLine = true, enabled = !busy, visualTransformation = PasswordVisualTransformation(),
                )
                OutlinedTextField(
                    cookie, { cookie = it }, label = { Text("Cookie сессии") },
                    singleLine = true, enabled = !busy, visualTransformation = PasswordVisualTransformation(),
                )
                OutlinedTextField(userId, { userId = it }, label = { Text("ID пользователя (необязательно)") }, singleLine = true, enabled = !busy)
                if (error) ErrorNotice("Подключить сессию не удалось.")
            }
        },
        confirmButton = {
            Button(
                enabled = !busy && token.trim().length >= 16 && cookie.isNotBlank(),
                onClick = {
                    val session = Session(
                        access_token = token.trim(),
                        user_id = userId.trim().toLongOrNull(),
                        api_user_agent = "KateMobileAndroid/56 lite-460 (Android 4.4.2; SDK 19; x86; unknown Android SDK built for x86; ru)",
                        refresh_cookie = cookie.trim(),
                        web_user_agent = "Mozilla/5.0",
                    )
                    busy = true
                    error = false
                    scope.launch {
                        val result = withContext(Dispatchers.Default) { runCatching { platformSignIn(session) } }
                        busy = false
                        if (result.isSuccess) {
                            onSessionChanged()
                            onDismiss()
                        } else error = true
                    }
                },
            ) { Text(if (busy) "Подключение…" else "Войти") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text("Отмена") } },
    )
}

@Composable
fun SearchScreen(
    catalog: CatalogViewModel,
    onPlayNext: ((Track) -> Unit)? = null,
    onPlayLast: ((Track) -> Unit)? = null,
    onPlay: (List<Track>, Int) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var submitted by remember { mutableStateOf("") }
    val tracks by catalog.searchResults.collectAsState()
    val searching by catalog.searching.collectAsState()
    val error by catalog.error.collectAsState()
    val focus = LocalFocusManager.current
    fun submit() {
        submitted = query.trim()
        focus.clearFocus()
        catalog.search(submitted)
    }

    LaunchedEffect(catalog) { catalog.search("") }
    CatalogColumn("Поиск", error) {
        item {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("Трек или исполнитель") },
                shape = RoundedCornerShape(17.dp),
                trailingIcon = { TextButton(onClick = ::submit) { Text("Найти") } },
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { submit() }),
            )
        }
        if (searching) {
            item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        } else if (tracks.isNotEmpty()) {
            item {
                Text(if (submitted.isBlank()) "Рекомендации" else "Результаты", Modifier.padding(top = 10.dp),
                    style = MaterialTheme.typography.titleLarge)
            }
        } else if (error == null && submitted.isNotBlank()) {
            item { EmptyState(MusicIcon.Search, "Совпадений нет", "Попробуй другой запрос.") }
        }
        itemsIndexed(tracks) { index, track ->
            TrackRow(track, active = false, onClick = { onPlay(tracks, index) }) {
                TrackActions(track, onPlayNext, onPlayLast, "Скачать трек", { catalog.download(track) })
            }
        }
    }
}

@Composable
fun MixesScreen(catalog: CatalogViewModel, onPlayMix: (MixLaunch) -> Unit) {
    val mixes by catalog.mixes.collectAsState()
    val releases by catalog.releases.collectAsState()
    val loading by catalog.loading.collectAsState()
    val error by catalog.error.collectAsState()
    LaunchedEffect(catalog) { catalog.refreshMixes() }

    LazyVerticalGrid(
        columns = GridCells.Adaptive(148.dp),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(20.dp, 24.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(span = { GridItemSpan(maxLineSpan) }) { PageTitle("Миксы", Modifier.padding(bottom = 6.dp)) }
        if (error != null) item(span = { GridItemSpan(maxLineSpan) }) { error?.let { ErrorNotice(it) } }
        if (loading) item(span = { GridItemSpan(maxLineSpan) }) { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        items(mixes) { mix ->
            CatalogTile(
                mix.title, mix.subtitle, mix.artwork_url, mix.id,
                onClick = if (loading) null else { { catalog.playMix(mix, onPlayMix) } },
            )
        }
        if (releases.isNotEmpty()) {
            item(span = { GridItemSpan(maxLineSpan) }) {
                Text("Новые релизы", Modifier.padding(top = 14.dp), style = MaterialTheme.typography.titleLarge)
            }
        }
        items(releases) { album ->
            CatalogTile(album.title, album.artists.joinToString { it.name }, album.artwork_url, "${album.owner_id}_${album.album_id}")
        }
    }
}

@Composable
private fun CatalogTile(title: String, detail: String, artwork: String?, key: String, onClick: (() -> Unit)? = null) {
    val shape = RoundedCornerShape(18.dp)
    Column(
        Modifier.fillMaxWidth().clip(shape).background(Panel)
            .then(if (onClick == null) Modifier else Modifier.clickable(onClickLabel = "Воспроизвести", onClick = onClick))
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(Modifier.fillMaxWidth().aspectRatio(1.45f)) {
            CoverArt(Modifier.matchParentSize(), artwork, key, radius = 11)
            if (onClick != null) {
                Box(
                    Modifier.align(Alignment.BottomEnd).padding(7.dp).size(32.dp)
                        .clip(RoundedCornerShape(10.dp)).background(Highlight),
                    contentAlignment = Alignment.Center,
                ) {
                    Glyph(MusicIcon.Play, Modifier.size(16.dp), MaterialTheme.colorScheme.onPrimary)
                }
            }
        }
        Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (detail.isNotBlank()) {
            Text(detail, color = TextMuted, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
fun DownloadsScreen(
    catalog: CatalogViewModel,
    onPlayNext: ((Track) -> Unit)? = null,
    onPlayLast: ((Track) -> Unit)? = null,
    onPlay: (List<Track>, Int) -> Unit,
) {
    val downloads by catalog.downloads.collectAsState()
    val error by catalog.error.collectAsState()
    val tracks = remember(downloads) { downloads.map { it.track } }
    var pendingDelete by remember { mutableStateOf<Track?>(null) }
    LaunchedEffect(catalog) { catalog.refreshDownloads() }
    CatalogColumn("Загрузки", error) {
        if (tracks.isEmpty()) {
            item { EmptyState(MusicIcon.Download, "Загрузок пока нет", "Сохрани трек, чтобы слушать его без интернета.") }
        }
        itemsIndexed(tracks) { index, track ->
            TrackRow(
                track, active = false, onClick = { onPlay(tracks, index) },
                endAction = {
                    TrackActions(track, onPlayNext, onPlayLast, "Удалить загрузку", { pendingDelete = track })
                },
            )
        }
    }
    pendingDelete?.let { track ->
        AlertDialog(
            onDismissRequest = { pendingDelete = null },
            title = { Text("Удалить загрузку?") },
            text = { Text(track.title) },
            confirmButton = {
                TextButton(onClick = {
                    catalog.removeDownload(track)
                    pendingDelete = null
                }) { Text("Удалить") }
            },
            dismissButton = { TextButton(onClick = { pendingDelete = null }) { Text("Отмена") } },
        )
    }
}

@Composable
fun LyricsScreen(
    track: Track?,
    catalog: CatalogViewModel,
    position: Double = 0.0,
    onBack: (() -> Unit)? = null,
    onSeek: ((Double) -> Unit)? = null,
) {
    val result by catalog.lyrics.collectAsState()
    val currentResult = result?.takeIf { it.trackKey == track?.key }
    val lyrics = currentResult?.lyrics
    val error = currentResult?.error
    LaunchedEffect(track?.key) { if (track != null) catalog.lyrics(track) }
    val lines = lyrics?.lines.orEmpty()
    val currentLine = lines.indexOfLast { it.time <= position }
    CatalogColumn("Текст песни", error, onBack) {
        if (track == null) {
            item { EmptyState(MusicIcon.Note, "Трек не выбран", "Открой плеер и выбери трек.") }
        } else {
            item {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    CoverArt(Modifier.size(54.dp), track.artwork_url, track.key, radius = 11)
                    Column {
                        Text(track.title, style = MaterialTheme.typography.titleLarge)
                        Text(track.artist, color = TextMuted, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
            if (currentResult == null) {
                item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
            } else if (lyrics == null && error == null) {
                item { Text("Текст пока недоступен", color = TextMuted) }
            }
            itemsIndexed(lines) { index, line ->
                Text(
                    line.text,
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp))
                        .background(if (index == currentLine) SelectedPanel else AppBackground)
                        .then(if (onSeek == null) Modifier else Modifier.clickable(onClickLabel = "Перейти к строке") { onSeek(line.time) })
                        .padding(12.dp),
                    color = if (index == currentLine) Highlight else TextMuted,
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            if (lyrics != null && lines.isEmpty()) {
                item { Text(lyrics.text, color = TextStrong, style = MaterialTheme.typography.bodyLarge) }
            }
        }
    }
}

@Composable
private fun CatalogColumn(
    title: String,
    error: String? = null,
    onBack: (() -> Unit)? = null,
    content: LazyListScope.() -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val gutter = if (maxWidth >= 620.dp) 34.dp else 20.dp
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(gutter, 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item {
                Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (onBack != null) GlyphButton(MusicIcon.Back, "Назад", onBack)
                    PageTitle(title)
                }
            }
            if (error != null) item { ErrorNotice(error) }
            content()
        }
    }
}
