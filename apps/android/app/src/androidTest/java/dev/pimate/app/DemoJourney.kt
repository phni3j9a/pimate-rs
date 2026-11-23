package dev.pimate.app

import android.graphics.Bitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.*
import org.junit.Assert.assertTrue

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

    // キーボードの出入りで配置が動いている間も外さないよう、座標のタップではなく操作そのものを呼ぶ。
    private fun press(node: SemanticsNodeInteraction) =
        node.performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)

    private fun waitFor(text: String, partial: Boolean = false) {
        ui.waitUntil(20000) {
            ui.onAllNodesWithText(text, substring = partial).fetchSemanticsNodes().isNotEmpty()
        }
    }

    @Test
    fun chat() =
        try {
            journey()
        } catch (e: Throwable) {
            shot("failure")
            throw e
        }

    @Test
    fun twoMachines() {
        val links = File(inst.targetContext.cacheDir, "two-links")
        Assume.assumeTrue("requires two disposable Hosts", links.exists())
        val pair = links.readLines().also { links.delete() }
        for ((i, link) in pair.withIndex()) {
            if (i > 0) {
                ui.onNodeWithContentDescription("設定").performClick()
                ui.onNodeWithText("マシンを追加（ペアリング）").performClick()
            }
            ui.onNodeWithText("リンクを貼り付ける").performClick()
            ui.onNodeWithContentDescription("ペアリング用のリンク").performTextInput(link)
            press(ui.onNodeWithText("接続"))
            waitFor("ログイン画面の改修")
        }
        ui.waitUntil(20000) { ui.onAllNodesWithText("ログイン画面の改修").fetchSemanticsNodes().size == 2 }
        shot("global-attention")
        ui.onNodeWithContentDescription("Instance・フォルダを検索").performTextInput("/demo/android")
        ui.waitUntil { ui.onAllNodesWithText("/demo/android").fetchSemanticsNodes().size == 2 }
        ui.onAllNodesWithText("接続中").assertCountEquals(2)
        shot("two-machines")
        ui.onAllNodesWithText("/demo/android")[0].performClick()
        waitFor("greeting", true)
        ui.onNodeWithContentDescription("メッセージ").performTextInput("host A")
        ui.onNodeWithContentDescription("Instance 一覧を開く").performClick()
        ui.onAllNodesWithText("/demo/android")[1].performClick()
        waitFor("greeting", true)
        ui.onNodeWithText("host A").assertDoesNotExist()
        ui.onNodeWithContentDescription("Instance 一覧を開く").performClick()
        ui.onAllNodesWithText("/demo/android")[0].performClick()
        waitFor("host A")
    }

    private fun journey() {
        val secret = File(inst.targetContext.cacheDir, "demo-link")
        Assume.assumeTrue("requires disposable Host", secret.exists())
        val link = secret.readText().trim()
        secret.delete()
        waitFor("PC の Pi とつなぐ")
        shot("pair")
        ui.onNodeWithText("リンクを貼り付ける").performClick()
        ui.onNodeWithContentDescription("ペアリング用のリンク").performTextInput(link)
        press(ui.onNodeWithText("接続"))
        waitFor("/demo/android")
        shot("drawer")
        ui.onNodeWithContentDescription("設定").performClick()
        ui.onNodeWithText("マシンを追加（ペアリング）").performClick()
        waitFor("マシンを追加")
        ui.activityRule.scenario.recreate()
        ui.onNodeWithText("マシンを追加").assertIsDisplayed()
        ui.runOnUiThread { ui.activity.onBackPressedDispatcher.onBackPressed() }
        ui.onNodeWithContentDescription("Instance 一覧を開く").performClick()
        ui.onNodeWithText("/demo/android").performClick()
        waitFor("greeting", true)
        shot("stream")
        waitFor("PiMate", true)
        ui.waitUntil(20000) {
            ui.onAllNodesWithContentDescription("送信").fetchSemanticsNodes().isNotEmpty()
        }
        val message = "日本語で確認します"
        ui.onNodeWithContentDescription("メッセージ").performTextInput(message)
        val orientation = ui.activity.requestedOrientation
        try {
            ui.runOnUiThread {
                ui.activity.requestedOrientation =
                    android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
            }
            ui.waitUntil(5000) {
                ui.activity.resources.configuration.orientation ==
                    android.content.res.Configuration.ORIENTATION_LANDSCAPE
            }
            ui.onNodeWithText(message).assertExists()
        } finally {
            ui.runOnUiThread { ui.activity.requestedOrientation = orientation }
        }
        ui.waitUntil(5000) {
            ui.activity.resources.configuration.orientation !=
                android.content.res.Configuration.ORIENTATION_LANDSCAPE
        }
        ui.onNodeWithText(message).assertIsDisplayed()
        ui.onNodeWithContentDescription("送信").assertIsDisplayed()
        shot("rotated")
        press(ui.onNodeWithContentDescription("送信"))
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
        val steer = "割り込みのテスト"
        ui.onNodeWithContentDescription("メッセージ").performTextInput(steer)
        press(ui.onNodeWithContentDescription("割り込む"))
        waitFor("割り込み待ち")
        shot("queued")
        press(ui.onNodeWithContentDescription("停止"))
        waitFor("下書きへ戻す")
        shot("unconfirmed")
        ui.onNodeWithText("下書きへ戻す").performClick()
        ui.onNode(hasSetTextAction() and hasText(steer)).assertExists()
        shot("restored")
        press(ui.onNodeWithContentDescription("送信"))
        // 回答を待つ間に履歴を遡っていても、届いた質問のカードへ戻る。
        val sent = hasText(steer) and !hasSetTextAction()
        ui.waitUntil(15000) { ui.onAllNodes(sent).fetchSemanticsNodes().isNotEmpty() }
        repeat(3) { ui.onNodeWithTag("chat").performTouchInput { swipeDown() } }
        assertTrue(ui.onNode(sent).isNotDisplayed())
        waitFor("作業ディレクトリの差分を確認します。")
        ui.onNodeWithText("はい").assertIsDisplayed()
        shot("question")
        ui.onNodeWithText("はい").performClick()
        ui.waitUntil(5000) { ui.onAllNodesWithText("はい").fetchSemanticsNodes().isEmpty() }
    }
}
