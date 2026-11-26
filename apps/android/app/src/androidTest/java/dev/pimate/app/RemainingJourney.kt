package dev.pimate.app

import android.graphics.Bitmap
import android.view.KeyEvent
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.*

/**
 * A fresh install and one single-use link for a disposable Host. Never run actions on saved hosts.
 */
class RemainingJourney {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()
    private val inst
        get() = InstrumentationRegistry.getInstrumentation()

    private fun find(m: SemanticsMatcher): SemanticsNodeInteraction {
        ui.waitUntil(25000) { ui.onAllNodes(m).fetchSemanticsNodes().isNotEmpty() }
        return ui.onAllNodes(m).onLast()
    }

    private fun tap(s: String, partial: Boolean = false) {
        find(hasText(s, substring = partial) and hasClickAction())
            .performSemanticsAction(SemanticsActions.OnClick)
    }

    private fun icon(s: String) {
        find(hasContentDescription(s)).performSemanticsAction(SemanticsActions.OnClick)
    }

    private fun back() {
        inst.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        ui.waitForIdle()
    }

    private fun shot(s: String) {
        ui.waitForIdle()
        android.os.SystemClock.sleep(450)
        File(inst.targetContext.cacheDir, "20-$s.png").outputStream().use {
            inst.uiAutomation.takeScreenshot().compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    private fun link(name: String): String {
        val f = File(inst.targetContext.cacheDir, name)
        Assume.assumeTrue("requires isolated test Host", f.exists())
        return f.readText().trim().also { f.delete() }
    }

    private fun pair(secret: String) {
        tap("リンクを貼り付ける")
        find(hasContentDescription("ペアリング用のリンク")).performTextInput(secret)
        tap("接続")
        tap("ペアリングする")
    }

    @Test
    fun loginHoldRecoversAfterDisconnect() {
        pair(link("reconnect-link"))
        find(hasText("選ぶとチャットを開きます。"))
        tap("一覧を開く")
        icon("設定")
        tap("デモ PC")
        val model = ui.runOnIdle { ViewModelProvider(ui.activity)[SessionModel::class.java] }
        val id = model.hosts.single().id
        val before = model.rowsFor(id).connectionGeneration
        tap("プロバイダーのログイン")
        find(hasText("openai"))
        // Like the browser covering the activity: the listener runs, but Compose gets no frames.
        ui.mainClock.autoAdvance = false
        try {
            ui.waitUntil(25000) {
                val now = model.rowsFor(id)
                now.connectionGeneration !== before &&
                    now.groups["host"].orEmpty().any {
                        it.id == "connection" && it.json == "\"connected\""
                    } &&
                    now.groups["provider"].isNullOrEmpty()
            }
            ui.onNodeWithText("openai").assertExists()
        } finally {
            ui.mainClock.autoAdvance = true
        }
        find(hasText("anthropic"))
        find(hasText("API キーを設定"))
    }

    /** `android-demo.py` の list fixture。起動はせず、選択・補完・保存だけを見る。 */
    @Test
    fun newPiAndDrawer() {
        pair(link("new-pi-link"))
        find(hasText("選ぶとチャットを開きます。"))
        shot("Parity-First")
        tap("ログイン画面の改修")
        icon("Instance 一覧を開く")
        find(hasText("要対応"))
        shot("Parity-Drawer")
        icon("新しい Pi を起動")
        find(hasText("Pi の既定（Opus 5.5 · medium）"))
        ui.onNodeWithText("起動").assertIsNotEnabled()
        shot("Parity-New")
        tap("モデル")
        find(hasText("お気に入り 3"))
        ui.onAllNodesWithText("Grok 5").assertCountEquals(0)
        tap("すべて 8")
        find(hasContentDescription("モデルを検索")).performTextInput("gRoK")
        find(hasText("Grok 5"))
        ui.onAllNodesWithText("Claude Opus 5.5").assertCountEquals(0)
        tap("Grok 5")
        tap("high")
        tap("完了")
        find(hasText("Grok 5 · high"))
        val path = find(hasContentDescription("フォルダのパス"))
        path.performTextInput("~/S")
        tap("src/")
        path.assertTextContains("~/src/")
        find(hasText("test/"))
        val toggle = find(hasContentDescription("質問ツールを使う"))
        toggle.assertIsOff().performSemanticsAction(SemanticsActions.OnClick)
        icon("閉じる")
        icon("Instance 一覧を開く")
        icon("新しい Pi を起動")
        find(hasContentDescription("質問ツールを使う")).assertIsOn()
        icon("閉じる")
        icon("Instance 一覧を開く")
        icon("設定")
        tap("マシンを追加（ペアリング）")
        find(hasText("PC の準備"))
        shot("Parity-Pair")
    }

    @Test
    fun firstEmpty() {
        pair(link("empty-link"))
        find(hasText("まだ起動中", substring = true))
        shot("First-Step-Empty")
        tap("新しい Pi を起動")
        find(hasContentDescription("フォルダのパス"))
    }

    @Test
    fun screens() {
        val secret = link("ui-link")
        shot("Pair")
        tap("スマホと PC を Tailscale でつなぐ")
        shot("Setup-Tailscale")
        tap("次へ：PC に PiMate を入れる")
        shot("Setup-PC")
        back()
        pair(secret)
        find(hasText("選ぶとチャットを開きます。"))
        shot("First-Step")
        tap("一覧を開く")
        icon("設定")
        find(hasText("テーマ"))
        shot("Settings")
        tap("デモ PC")
        find(hasText("ペアリング済みの端末"))
        shot("Machine-Detail")
        val fixture = File(inst.targetContext.cacheDir, "mock-maintenance").delete()
        if (fixture) {
            tap("更新")
            find(hasText("実行時の最新版に更新します（確認時点の最新版: 0.1.20）", substring = true))
            tap("更新")
            find(hasText("0.1.20 に更新しました。実行中の各 Pi で /reload を実行してください。"))
            tap("Host を再起動")
            tap("Host を再起動")
            find(hasText("再起動を受け付けました"))
            tap("プロバイダーのログイン")
            find(hasText("API キーを設定"))
            shot("Provider")
            tap("API キーを設定")
            find(hasSetTextAction() and hasText("API キー")).performTextInput("fixture-only")
            tap("送信")
            find(hasText("ログインしました"))
            tap("ログアウト")
            tap("キャンセル")
            tap("ログアウト")
            tap("ログアウト")
            find(hasText("未設定"))
            icon("戻る")
        }
        tap("このマシンを削除")
        tap("キャンセル")
        icon("戻る")
        icon("戻る")
        icon("Instance 一覧を開く")
        icon("新しい Pi を起動")
        find(hasText("モデル"))
        shot("New-Pi")
        back()
        icon("Instance 一覧を開く")
        tap("--shout フラグを追加")
        icon("セッションのメニュー")
        tap("変更レビュー")
        find(hasText("この変更について依頼"))
        shot("Review")
        tap("セッション全体")
        tap("この変更について依頼")
        find(hasText("このセッションの変更", substring = true) and hasSetTextAction())
        icon("セッションのメニュー")
        tap("GitHub の Issue / PR")
        find(hasText("GitHub"))
        ui.waitUntil(25000) {
            ui.onAllNodes(hasText("PC から読み込み中…")).fetchSemanticsNodes().isEmpty()
        }
        shot("GitHub")
        if (fixture) find(hasText("example/demo"))
        if (fixture || ui.onAllNodes(hasText("example/demo")).fetchSemanticsNodes().isNotEmpty()) {
            tap("#20", true)
            find(hasText("画面の動作と表示", substring = true))
            shot("GitHub-Issue")
            icon("戻る")
            tap("PR")
            tap("#20", true)
            find(hasText("画面の動作と表示", substring = true))
            find(hasText("CI チェックの一部を省略", substring = true))
            find(hasText("GitHub で開く"))
            shot("GitHub-PR")
            tap("Pi に依頼")
            find(
                hasSetTextAction() and
                    hasText("このセッションの変更", substring = true) and
                    hasText("#20「画面の実装」", substring = true)
            )
        } else icon("戻る")
        icon("Instance 一覧を開く")
        tap("認証まわりのレビュー")
        find(hasText("レビュアーをバックグラウンドで起動しました。", substring = true))
        shot("Chat-Subagents")
        tap("サブエージェント  ", true)
        find(hasText("以前の実行"))
        shot("Sheet-Subagents")
        tap("認証")
        find(hasText("認証の期限切れ判定", substring = true))
        shot("Subagent-Detail")
        find(hasSetTextAction() and hasText("指示")).performTextInput("テストへの影響も確認してください")
        tap("割り込む")
        find(hasText("あなた · 割り込む"))
        tap("この子を停止…")
        tap("キャンセル")
        icon("戻る")
        tap("ログイン処理の調査")
        find(hasText("見るだけ", substring = true))
        shot("Subagent-Detail-Readonly")
    }

    /** For the isolated real Pi + faux provider only. The normal demo must not match this PC. */
    @Test
    fun realFaux() {
        pair(link("real-link"))
        find(hasText("検証用 PC とつながりました"))
        tap("work")
        find(hasContentDescription("メッセージ")).performTextInput("say hello")
        icon("送信")
        find(hasText("Hello from the PiMate faux model.", substring = true))
        icon("Instance 一覧を開く")
        icon("新しい Pi を起動")
        val folder = find(hasContentDescription("フォルダのパス") and hasSetTextAction())
        ui.onNodeWithText("起動").assertIsNotEnabled()
        folder.performTextInput("~/")
        tap("起動")
        ui.waitUntil(30000) {
            ui.onAllNodes(hasContentDescription("フォルダのパス")).fetchSemanticsNodes().isEmpty()
        }
        find(hasContentDescription("セッションのメニュー"))
        icon("Instance 一覧を開く")
        icon("設定")
        tap("検証用 PC")
        find(hasText("接続中", substring = true))
        find(hasText("ペアリング済みの端末"))
    }
}
