package dev.pimate.app

import android.graphics.Bitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.*

class DemoJourney {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()
    private val inst
        get() = InstrumentationRegistry.getInstrumentation()

    private fun shot(name: String) {
        ui.waitForIdle()
        inst.waitForIdleSync()
        android.os.SystemClock.sleep(400) // Allow the device compositor to present the frame.
        File(inst.targetContext.cacheDir, "$name.png").outputStream().use {
            inst.uiAutomation.takeScreenshot().compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    private fun waitFor(text: String, partial: Boolean = false) {
        ui.waitUntil(20000) {
            ui.onAllNodesWithText(text, substring = partial).fetchSemanticsNodes().isNotEmpty()
        }
    }

    @Test
    fun chat() {
        val secret = File(inst.targetContext.cacheDir, "demo-link")
        Assume.assumeTrue("requires disposable Host", secret.exists())
        val link = secret.readText().trim()
        secret.delete()
        waitFor("PC とペアリング")
        shot("pair")
        ui.onNodeWithText("ペアリングリンク").performTextInput(link)
        ui.onNodeWithText("接続").performClick()
        waitFor("/demo/android")
        shot("drawer")
        ui.onNodeWithText("/demo/android").performClick()
        waitFor("greeting", true)
        shot("stream")
        waitFor("送信")
        waitFor("PiMate", true)
        val message = "日本語で確認します"
        ui.onNodeWithText("メッセージ").performTextInput(message)
        ui.onNodeWithText("送信").performClick()
        waitFor(message)
        ui.waitUntil(15000) {
            ui.onAllNodes(hasText(message) and !hasSetTextAction())
                .fetchSemanticsNodes()
                .isNotEmpty()
        }
        ui.runOnUiThread {
            ui.activity
                .getSystemService(android.view.inputmethod.InputMethodManager::class.java)
                .hideSoftInputFromWindow(ui.activity.window.decorView.windowToken, 0)
        }
        shot("sent")
    }
}
