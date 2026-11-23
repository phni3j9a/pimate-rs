package dev.pimate.app

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.*
import androidx.compose.ui.text.TextStyle

val gutter = 16.dp
@Composable fun PiTheme(scale: Float, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    val colors = if (isSystemInDarkTheme()) darkColorScheme(primary = Color(0xff80c0df), surface = Color(0xff1f2326), onSurface = Color(0xfff1f3f4))
        else lightColorScheme(primary = Color(0xff1b6b91), surface = Color.White, onSurface = Color(0xff1f2326), onSurfaceVariant = Color(0xff5e686d), error = Color(0xffa84d45))
    CompositionLocalProvider(LocalDensity provides Density(density.density, density.fontScale * scale)) {
        MaterialTheme(colorScheme = colors, typography = Typography(bodyLarge = TextStyle(fontSize = 14.sp,lineHeight = 21.sp), titleMedium = TextStyle(fontSize = 15.sp), labelSmall = TextStyle(fontSize = 11.sp)), content = content)
    }
}
