package com.dec.privatemusic.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.dec.privatemusic.player.*

@Composable
fun ProfileScreen(session: SessionState, onLogin: (() -> Unit)?, onLogout: () -> Unit, onSessionChanged: () -> Unit) {
    var manualLogin by remember { mutableStateOf(false) }
    CatalogColumn("Профиль") {
        item {
            Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(if (session.signed_in) "Аккаунт VK подключён" else "Требуется вход", fontWeight = FontWeight.SemiBold, fontSize = 20.sp)
                    if (session.signed_in) {
                        Text("${session.profile?.first_name.orEmpty()} ${session.profile?.last_name.orEmpty()}".trim())
                        Text("Сессия сохраняется в защищённом хранилище", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    } else {
                        Text("Войдите через VK, чтобы открыть библиотеку, миксы и рекомендации", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if (session.signed_in) {
                        Button(onClick = onLogout) { Text("Выйти") }
                    } else {
                        Button(onClick = { if (onLogin != null) onLogin() else manualLogin = true }) { Text("Войти через VK") }
                    }
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
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Сессия VK") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("Для desktop можно вставить токен и cookie из текущей сессии VK")
                OutlinedTextField(value = token, onValueChange = { token = it }, label = { Text("Access token") }, singleLine = true)
                OutlinedTextField(value = cookie, onValueChange = { cookie = it }, label = { Text("Refresh cookie") })
                OutlinedTextField(value = userId, onValueChange = { userId = it }, label = { Text("User ID (необязательно)") }, singleLine = true)
            }
        },
        confirmButton = {
            TextButton(onClick = {
                if (token.length >= 16 && cookie.isNotBlank()) {
                    platformSignIn(Session(
                        access_token = token,
                        user_id = userId.toLongOrNull(),
                        api_user_agent = "KateMobileAndroid/56 lite-460 (Android 4.4.2; SDK 19; x86; unknown Android SDK built for x86; ru)",
                        refresh_cookie = cookie,
                        web_user_agent = "Mozilla/5.0",
                    ))
                    onSessionChanged()
                    onDismiss()
                }
            }) { Text("Войти") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Отмена") } },
    )
}

@Composable
fun SearchScreen(catalog: CatalogViewModel, onPlay: (List<Track>, Int) -> Unit) {
    var query by remember { mutableStateOf("") }
    val tracks by catalog.searchResults.collectAsState()
    LaunchedEffect(catalog) { catalog.search("") }
    CatalogColumn("Поиск по VK", error = catalog.error.collectAsState().value) {
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(value = query, onValueChange = { query = it }, modifier = Modifier.weight(1f), singleLine = true, label = { Text("Трек или исполнитель") })
                Button(onClick = { catalog.search(query) }) { Text("Найти") }
            }
        }
        itemsIndexed(tracks, key = { _, track -> track.key }) { index, track ->
            TrackRow(track = track, active = false, onClick = { onPlay(tracks, index) }, onDownload = { catalog.download(track) })
        }
    }
}

@Composable
fun MixesScreen(catalog: CatalogViewModel, onPlayMix: (MixLaunch) -> Unit) {
    val mixes by catalog.mixes.collectAsState()
    val releases by catalog.releases.collectAsState()
    val loading by catalog.loading.collectAsState()
    LaunchedEffect(catalog) { catalog.start() }
    CatalogColumn("Миксы и релизы", error = catalog.error.collectAsState().value) {
        if (loading) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        items(mixes, key = { "${it.id}#${it.options}" }) { mix ->
            Card(Modifier.fillMaxWidth().clickable(enabled = !loading) { catalog.playMix(mix, onPlayMix) }, colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    CoverArt(Modifier.size(48.dp))
                    Column(Modifier.padding(start = 12.dp)) {
                        Text(mix.title, fontWeight = FontWeight.SemiBold)
                        Text(mix.subtitle, color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 13.sp)
                    }
                }
            }
        }
        if (releases.isNotEmpty()) item { Text("Новые релизы", fontSize = 20.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(top = 16.dp)) }
        items(releases, key = { "${it.owner_id}_${it.album_id}" }) { album ->
            Card(Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                Column(Modifier.padding(16.dp)) {
                    Text(album.title, fontWeight = FontWeight.SemiBold)
                    Text(album.artists.joinToString { it.name }, color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 13.sp)
                }
            }
        }
    }
}

@Composable
fun DownloadsScreen(catalog: CatalogViewModel, onPlay: (List<Track>, Int) -> Unit) {
    val downloads by catalog.downloads.collectAsState()
    LaunchedEffect(catalog) { catalog.refreshDownloads() }
    CatalogColumn("Загрузки", error = catalog.error.collectAsState().value) {
        if (downloads.isEmpty()) item { Text("Пока ничего не загружено", color = MaterialTheme.colorScheme.onSurfaceVariant) }
        itemsIndexed(downloads, key = { _, item -> item.track.key }) { index, item ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f)) {
                    TrackRow(track = item.track, active = false, onClick = { onPlay(downloads.map { it.track }, index) })
                }
                TextButton(onClick = { catalog.removeDownload(item.track) }) { Text("Удалить") }
            }
        }
    }
}

@Composable
fun LyricsScreen(track: Track?, catalog: CatalogViewModel) {
    val lyrics by catalog.lyrics.collectAsState()
    LaunchedEffect(track?.key) { if (track != null) catalog.lyrics(track) }
    CatalogColumn("Текст песни", error = catalog.error.collectAsState().value) {
        if (track == null) item { Text("Выберите трек") }
        else {
            item { Text("${track.artist} — ${track.title}", fontWeight = FontWeight.SemiBold, fontSize = 20.sp) }
            if (lyrics == null) item { Text("Текст не найден", color = MaterialTheme.colorScheme.onSurfaceVariant) }
            lyrics?.lines?.forEach { line ->
                item { Text(line.text, fontSize = 18.sp, modifier = Modifier.padding(vertical = 6.dp)) }
            }
            if (lyrics != null && lyrics?.lines.isNullOrEmpty()) {
                item { Text(lyrics?.text.orEmpty(), fontSize = 18.sp, modifier = Modifier.padding(vertical = 6.dp)) }
            }
        }
    }
}

@Composable
private fun CatalogColumn(title: String, error: String? = null, content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(20.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item { Text(title, fontSize = 28.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(bottom = 12.dp)) }
        if (error != null) item { Text(error, color = MaterialTheme.colorScheme.error) }
        content()
    }
}
