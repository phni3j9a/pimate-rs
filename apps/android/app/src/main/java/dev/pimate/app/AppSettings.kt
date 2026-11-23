package dev.pimate.app

import android.content.SharedPreferences
import androidx.compose.runtime.*

enum class ThemeMode {
    SYSTEM,
    LIGHT,
    DARK;

    fun dark(system: Boolean) = this == DARK || this == SYSTEM && system
}

internal class AppSettings(private val prefs: SharedPreferences) {
    var theme by
        mutableStateOf(
            ThemeMode.entries.find { it.name == prefs.getString("theme", null) } ?: ThemeMode.SYSTEM
        )
        private set

    var scale by mutableFloatStateOf(prefs.getFloat("scale", 1f))
        private set

    var askTool by mutableStateOf(prefs.getBoolean("askTool", false))
        private set

    fun chooseTheme(value: ThemeMode) {
        prefs.edit().putString("theme", value.name).apply()
        theme = value
    }

    fun chooseScale(value: Float) {
        require(fontScales.any { it.second == value })
        prefs.edit().putFloat("scale", value).apply()
        scale = value
    }

    fun chooseAskTool(value: Boolean) {
        prefs.edit().putBoolean("askTool", value).apply()
        askTool = value
    }
}
