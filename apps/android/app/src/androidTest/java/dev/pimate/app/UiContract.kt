package dev.pimate.app

import android.content.Intent
import android.content.IntentFilter
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.*
import org.junit.Assert.*

class UiContract {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    private fun show(content: @Composable () -> Unit) =
        ui.runOnUiThread { ui.activity.setContent { PiTheme(1f) { content() } } }

    @Test
    fun stopConfirmationNeverExpandsOrTransfersItsTarget() {
        val editor = EditorKey("host", uniffi.pimate_ffi.Target("pi", "s"))
        val original = StopTarget("parallel", "a", "stop_child")
        var offered by mutableStateOf<StopTarget?>(original)
        var live: StopTarget? = original
        val sent = mutableListOf<StopTarget>()
        show { StopAgentAction(editor, offered, { live }) { sent += it } }
        ui.onNodeWithText("この子を停止…").performClick()
        ui.runOnIdle {
            live = original.copy(action = "stop")
            offered = live
        }
        ui.onNodeWithText("キャンセル").assertDoesNotExist()
        ui.onNodeWithText("実行全体を停止…").assertExists()
        ui.runOnIdle {
            live = original
            offered = live
        }
        ui.onNodeWithText("この子を停止…").performClick()
        ui.runOnIdle {
            live = null
            offered = null
        }
        ui.onNodeWithText("キャンセル").assertDoesNotExist()
        ui.onNodeWithText("実行全体を停止…").assertDoesNotExist()
        ui.runOnIdle {
            live = original
            offered = original
        }
        ui.onNodeWithText("この子を停止…").performClick()
        // Simulate a delivery already received, before the next composition.
        ui.runOnIdle { live = null }
        ui.onNode(hasText("この子を停止") and hasClickAction()).performClick()
        ui.runOnIdle {
            assertTrue(sent.isEmpty())
            live = original
            offered = original
        }
        ui.onNodeWithText("この子を停止…").performClick()
        ui.onNode(hasText("この子を停止") and hasClickAction()).performClick()
        ui.runOnIdle { assertEquals(listOf(original), sent) }
    }

    @Test
    fun drawerIsCappedOnWideScreens() {
        var width by mutableStateOf(800.dp)
        show {
            Box(Modifier.requiredWidth(width)) {
                ModalNavigationDrawer(
                    { DrawerSheet { Text("幅", Modifier.fillMaxWidth()) } },
                    drawerState = rememberDrawerState(DrawerValue.Open),
                ) {}
            }
        }
        ui.onNodeWithText("幅").assertWidthIsEqualTo(330.dp)
        ui.runOnIdle { width = 360.dp }
        ui.onNodeWithText("幅").assertWidthIsEqualTo(306.dp)
    }

    @Test
    fun tableLinksOpenOnlyInTheFullScreenTable() {
        // ブラウザを開く代わりに、開こうとした回数だけ数える（全画面は別の窓で、そこの UriHandler は差し替えられない）。
        val inst = InstrumentationRegistry.getInstrumentation()
        val browser =
            inst.addMonitor(
                IntentFilter(Intent.ACTION_VIEW).apply { addDataScheme("https") },
                null,
                true,
            )
        try {
            // セルいっぱいに折り返すリンク（タップの位置がリンクの文字に重なる）。
            val label = "セルの幅いっぱいに折り返す長いリンクの文字列".repeat(3)
            show { Markdown("| リンク |\n| --- |\n| [$label](https://example.com/) |\n") }
            ui.waitUntil(5000) { ui.onAllNodesWithText(label).fetchSemanticsNodes().isNotEmpty() }
            ui.onNodeWithText(label).performClick()
            ui.onNodeWithContentDescription("表を Markdown でコピー").assertExists()
            assertEquals(0, browser.hits)
            // 全画面ではリンクを開ける。
            ui.onAllNodesWithText(label)[1].performClick()
            ui.waitUntil(5000) { browser.hits == 1 }
        } finally {
            inst.removeMonitor(browser)
        }
    }

    @Test
    fun answersAreParsedOffTheUiThreadAndReusedWhenShownAgain() {
        val text = "解析 ${System.nanoTime()}"
        val source = "# 見出し\n\n$text"
        val shown = { ui.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().size }
        var again by mutableStateOf(false)
        // 時計を止めると、初回の合成のあとはフレームを進めるまで合成し直さない。
        ui.mainClock.autoAdvance = false
        show { if (again) key("again") { Markdown(source) } else Markdown(source) }
        // 最初の合成は解析を待たない。
        assertEquals(0, shown())
        ui.mainClock.autoAdvance = true
        ui.waitUntil(5000) { shown() == 1 }
        // 一度解析した回答は、新しく表示したとき最初の合成から出る（スクロールで戻ったとき）。
        ui.mainClock.autoAdvance = false
        ui.runOnIdle { again = true }
        ui.mainClock.advanceTimeByFrame()
        assertEquals(1, shown())
        ui.mainClock.autoAdvance = true
    }
}
