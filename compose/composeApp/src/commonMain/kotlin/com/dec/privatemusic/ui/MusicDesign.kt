package com.dec.privatemusic.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage

internal val AppBackground = Color(0xFF101517)
internal val Panel = Color(0xFF171E21)
internal val RaisedPanel = Color(0xFF20292D)
internal val SelectedPanel = Color(0xFF24362D)
internal val TextStrong = Color(0xFFF2F7F4)
internal val TextMuted = Color(0xFFA7B7B6)
internal val Highlight = Color(0xFFB8EB99)
internal val Border = Color(0xFF37464B)

private val MusicColors = darkColorScheme(
    primary = Highlight,
    onPrimary = Color(0xFF162412),
    primaryContainer = SelectedPanel,
    onPrimaryContainer = Highlight,
    secondary = Color(0xFF91CFC0),
    background = AppBackground,
    onBackground = TextStrong,
    surface = Panel,
    onSurface = TextStrong,
    surfaceVariant = RaisedPanel,
    onSurfaceVariant = TextMuted,
    surfaceContainer = Panel,
    surfaceContainerHigh = RaisedPanel,
    outline = Border,
    outlineVariant = Border,
    error = Color(0xFFFFB4A8),
)

@Composable
internal fun MusicTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = MusicColors, content = content)
}

@Composable
internal fun PageTitle(title: String, modifier: Modifier = Modifier) {
    Text(
        title,
        modifier.semantics { heading() },
        style = MaterialTheme.typography.headlineLarge,
        fontWeight = FontWeight.Bold,
    )
}

@Composable
internal fun EmptyState(icon: MusicIcon, title: String, detail: String) {
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(Panel).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Glyph(icon, Modifier.size(28.dp), Highlight)
        Text(title, style = MaterialTheme.typography.titleLarge)
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = TextMuted)
    }
}

@Composable
internal fun ErrorNotice(message: String) {
    Text(
        message,
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.error.copy(alpha = 0.10f)).padding(14.dp),
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodyMedium,
    )
}

private val CoverColors = listOf(
    Color(0xFF254640) to Color(0xFF102423),
    Color(0xFF3D4265) to Color(0xFF202638),
    Color(0xFF573D49) to Color(0xFF2C202B),
    Color(0xFF4E472F) to Color(0xFF2A281C),
)

@Composable
internal fun CoverArt(
    modifier: Modifier = Modifier,
    artwork: String? = null,
    key: String = "",
    radius: Int = 14,
) {
    val (light, dark) = CoverColors[(key.hashCode() and Int.MAX_VALUE) % CoverColors.size]
    val shape = RoundedCornerShape(radius.dp)
    BoxWithConstraints(
        modifier.clip(shape).background(Brush.linearGradient(listOf(light, dark)))
            .border(1.dp, Color.White.copy(alpha = 0.10f), shape),
        contentAlignment = Alignment.Center,
    ) {
        Glyph(MusicIcon.Note, Modifier.size(minOf(maxWidth, maxHeight) * 0.42f), TextStrong.copy(alpha = 0.72f))
        if (artwork?.startsWith("https://", ignoreCase = true) == true) {
            AsyncImage(
                model = artwork,
                contentDescription = null,
                modifier = Modifier.matchParentSize(),
                contentScale = ContentScale.Crop,
            )
        }
    }
}

internal enum class MusicIcon {
    Home, Search, Mix, Download, Profile, Note, Play, Pause, Previous, Next, Queue, Back, Expand, Volume, Trash, Check,
}

@Composable
internal fun GlyphButton(
    icon: MusicIcon,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    IconButton(onClick, modifier.semantics { contentDescription = label }, enabled = enabled) {
        Glyph(icon, Modifier.size(22.dp), if (enabled) TextStrong else TextMuted.copy(alpha = 0.45f))
    }
}

@Composable
internal fun Glyph(icon: MusicIcon, modifier: Modifier, color: Color = TextStrong) {
    Canvas(modifier) {
        scale(size.width / 24f, size.height / 24f, pivot = Offset.Zero) {
            when (icon) {
                MusicIcon.Home -> outline(color, 3f, 10f, 12f, 3f, 21f, 10f, 21f, 20f, 15f, 20f, 15f, 14f, 9f, 14f, 9f, 20f, 3f, 20f, 3f, 10f)
                MusicIcon.Search -> {
                    drawCircle(color, 6f, Offset(10.5f, 10.5f), style = Stroke(1.75f))
                    outline(color, 15f, 15f, 20f, 20f)
                }
                MusicIcon.Mix -> {
                    outline(color, 3f, 14f, 6f, 14f, 9f, 7f, 13f, 18f, 16f, 11f, 21f, 11f)
                    outline(color, 19f, 8f, 22f, 11f, 19f, 14f)
                }
                MusicIcon.Download -> {
                    outline(color, 12f, 3f, 12f, 15f)
                    outline(color, 8f, 11f, 12f, 15f, 16f, 11f)
                    outline(color, 4f, 16f, 4f, 20f, 20f, 20f, 20f, 16f)
                }
                MusicIcon.Profile -> {
                    drawCircle(color, 3.4f, Offset(12f, 7.5f), style = Stroke(1.75f))
                    drawPath(Path().apply { moveTo(5f, 21f); quadraticTo(5f, 14f, 12f, 14f); quadraticTo(19f, 14f, 19f, 21f) },
                        color, style = Stroke(1.75f, cap = StrokeCap.Round))
                }
                MusicIcon.Note -> {
                    outline(color, 11f, 16f, 11f, 4f, 19f, 2.5f, 19f, 13.5f)
                    drawOval(color, Offset(4f, 15f), Size(8f, 5f))
                    drawOval(color, Offset(12f, 12.5f), Size(8f, 5f))
                }
                MusicIcon.Play -> triangle(color, 7f, 4.5f, 19.5f, 12f, 7f, 19.5f)
                MusicIcon.Pause -> {
                    bar(color, 7f, 5f, 3.5f, 14f)
                    bar(color, 13.5f, 5f, 3.5f, 14f)
                }
                MusicIcon.Previous -> {
                    bar(color, 4f, 5f, 2.5f, 14f)
                    triangle(color, 18.5f, 5f, 7.5f, 12f, 18.5f, 19f)
                }
                MusicIcon.Next -> {
                    triangle(color, 5.5f, 5f, 16.5f, 12f, 5.5f, 19f)
                    bar(color, 17.5f, 5f, 2.5f, 14f)
                }
                MusicIcon.Queue -> {
                    outline(color, 4f, 6f, 20f, 6f)
                    outline(color, 4f, 12f, 20f, 12f)
                    outline(color, 4f, 18f, 13f, 18f)
                }
                MusicIcon.Back -> {
                    outline(color, 20f, 12f, 4f, 12f)
                    outline(color, 10f, 6f, 4f, 12f, 10f, 18f)
                }
                MusicIcon.Expand -> {
                    outline(color, 7f, 17f, 17f, 7f)
                    outline(color, 9f, 7f, 17f, 7f, 17f, 15f)
                }
                MusicIcon.Volume -> {
                    outline(color, 4f, 9f, 8f, 9f, 13f, 5f, 13f, 19f, 8f, 15f, 4f, 15f, 4f, 9f)
                    drawPath(Path().apply { moveTo(16f, 8f); quadraticTo(20f, 12f, 16f, 16f) },
                        color, style = Stroke(1.75f, cap = StrokeCap.Round))
                }
                MusicIcon.Trash -> {
                    outline(color, 4f, 6f, 20f, 6f)
                    outline(color, 9f, 6f, 9f, 3f, 15f, 3f, 15f, 6f)
                    outline(color, 6f, 6f, 7f, 21f, 17f, 21f, 18f, 6f)
                }
                MusicIcon.Check -> outline(color, 5f, 12f, 10f, 17f, 19f, 7f)
            }
        }
    }
}

private fun DrawScope.outline(color: Color, vararg points: Float) {
    val path = Path().apply {
        moveTo(points[0], points[1])
        for (index in 2 until points.size step 2) lineTo(points[index], points[index + 1])
    }
    drawPath(path, color, style = Stroke(1.75f, cap = StrokeCap.Round, join = StrokeJoin.Round))
}

private fun DrawScope.triangle(color: Color, x1: Float, y1: Float, x2: Float, y2: Float, x3: Float, y3: Float) {
    drawPath(Path().apply { moveTo(x1, y1); lineTo(x2, y2); lineTo(x3, y3); close() }, color)
}

private fun DrawScope.bar(color: Color, x: Float, y: Float, width: Float, height: Float) {
    drawRoundRect(color, Offset(x, y), Size(width, height), CornerRadius(0.8f))
}
