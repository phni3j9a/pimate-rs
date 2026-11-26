package dev.pimate.app

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.test.platform.app.InstrumentationRegistry
import java.util.UUID
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.Host

class SettingsContract {
    private val inst = InstrumentationRegistry.getInstrumentation()
    private val app
        get() = inst.targetContext.applicationContext as PiApplication

    @Test
    fun settingsUpdateImmediatelyAndSurviveReopen() {
        val name = "fixture-${UUID.randomUUID()}"
        val prefs = app.getSharedPreferences(name, 0)
        try {
            val settings = AppSettings(prefs)
            assertTrue(settings.theme.dark(true))
            assertFalse(settings.theme.dark(false))
            settings.chooseTheme(ThemeMode.DARK)
            assertTrue(settings.theme.dark(false))
            assertEquals(ThemeMode.DARK, AppSettings(prefs).theme)
            settings.chooseTheme(ThemeMode.LIGHT)
            assertFalse(settings.theme.dark(true))
            settings.chooseScale(1.15f)
            settings.chooseAskTool(true)
            val reopened = AppSettings(prefs)
            assertEquals(ThemeMode.LIGHT, reopened.theme)
            assertEquals(1.15f, reopened.scale, 0f)
            assertTrue(reopened.askTool)
        } finally {
            app.deleteSharedPreferences(name)
        }
    }

    @Test
    fun additionalPairRestoresInsteadOfEmittingStartupDrawer() {
        val id = UUID.randomUUID().toString()
        val host = Host(id, "fixture", "127.0.0.1:9", "local", false)
        val stores = mutableListOf<ViewModelStore>()
        fun model(state: SavedStateHandle): SessionModel {
            val store = ViewModelStore().also(stores::add)
            return ViewModelProvider(
                store,
                object : ViewModelProvider.Factory {
                    @Suppress("UNCHECKED_CAST")
                    override fun <T : ViewModel> create(modelClass: Class<T>): T =
                        SessionModel(app, state) as T
                },
            )[SessionModel::class.java]
        }
        inst.runOnMainSync {
            try {
                app.store.save(host, "fixture")
                val state = SavedStateHandle()
                val original = model(state)
                assertTrue(original.openList)
                original.listed()
                original.showPairing(true)
                stores.first().clear()
                val restored =
                    model(
                        SavedStateHandle(
                            mapOf("additionalPair" to state.get<Boolean>("additionalPair"))
                        )
                    )
                assertTrue(restored.pairing)
                assertFalse(restored.openList)
                restored.showPairing(false)
                restored.forget(host)
                assertFalse(restored.hosts.any { it.id == id })
                assertFalse(app.store.hosts().any { it.id == id })
            } finally {
                stores.forEach { it.clear() }
                app.store.forget(id)
            }
        }
    }
}
