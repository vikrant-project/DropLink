package com.droplink.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

val ObsidianBackground = Color(0xFF0B0F19)
val CardSurface = Color(0xFF111827)
val CardHover = Color(0xFF1F2937)
val ElectricIndigo = Color(0xFF4F46E5)
val AccentIndigo = Color(0xFF6366F1)
val EmeraldSuccess = Color(0xFF10B981)
val TextPrimary = Color(0xFFF9FAFB)
val TextMuted = Color(0xFF9CA3AF)

private val DarkColorScheme = darkColorScheme(
    primary = ElectricIndigo,
    secondary = AccentIndigo,
    background = ObsidianBackground,
    surface = CardSurface,
    onPrimary = Color.White,
    onSecondary = Color.White,
    onBackground = TextPrimary,
    onSurface = TextPrimary
)

private val LightColorScheme = lightColorScheme(
    primary = ElectricIndigo,
    secondary = AccentIndigo,
    background = Color(0xFFF9FAFB),
    surface = Color.White,
    onPrimary = Color.White,
    onSecondary = Color.White,
    onBackground = Color(0xFF111827),
    onSurface = Color(0xFF111827)
)

@Composable
fun DropLinkTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit
) {
    val colorScheme = if (darkTheme) DarkColorScheme else LightColorScheme
    MaterialTheme(
        colorScheme = colorScheme,
        content = content
    )
}
