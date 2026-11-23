package dev.pimate.app

import android.graphics.Bitmap
import android.view.KeyEvent
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.*

/**
 * シートと画像の操作を、模擬 Host（scripts/android-demo.py）で結果が返るところまで確かめる。 DemoJourney
 * と同じく、アプリのデータが空で、模擬 Host を起動し直した直後に実行する（リンクは cache/demo-link）。Pi の終了や セッションの切り替えを行うので、1
 * 回の模擬 Host につき 1 回だけ。
 */
class SheetJourney {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()
    private val inst
        get() = InstrumentationRegistry.getInstrumentation()

    private fun shot(name: String) {
        ui.waitForIdle()
        inst.waitForIdleSync()
        android.os.SystemClock.sleep(400)
        File(inst.targetContext.cacheDir, "$name.png").outputStream().use {
            inst.uiAutomation.takeScreenshot().compress(Bitmap.CompressFormat.PNG, 100, it)
        }
    }

    private fun press(node: SemanticsNodeInteraction) =
        node.performSemanticsAction(SemanticsActions.OnClick)

    private fun exists(matcher: SemanticsMatcher) =
        ui.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty()

    private fun waitFor(matcher: SemanticsMatcher) = ui.waitUntil(20000) { exists(matcher) }

    private fun waitFor(text: String, partial: Boolean = false) =
        waitFor(hasText(text, substring = partial))

    private fun gone(text: String) = ui.waitUntil(20000) { !exists(hasText(text)) }

    /** シートと全画面は別の窓。戻るキーで閉じる。 */
    private fun back() {
        ui.waitForIdle()
        inst.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        ui.waitForIdle()
    }

    /** 一覧の行（見出しの題と区別する）。 */
    private fun open(title: String) {
        val row = hasText(title) and SemanticsMatcher.keyIsDefined(SemanticsProperties.Selected)
        if (!exists(row)) press(ui.onNodeWithContentDescription("Instance 一覧を開く"))
        waitFor(row)
        press(ui.onNode(row))
        ui.waitUntil(20000) { exists(hasContentDescription("セッションのメニュー")) }
    }

    private fun menu(item: String) {
        press(ui.onNodeWithContentDescription("セッションのメニュー"))
        waitFor(item)
        press(ui.onNodeWithText(item))
    }

    private val field
        get() = ui.onNode(hasSetTextAction() and hasContentDescription("メッセージ"))

    @Test
    fun sheets() =
        try {
            journey()
        } catch (e: Throwable) {
            shot("sheet-failure")
            throw e
        }

    private fun journey() {
        val secret = File(inst.targetContext.cacheDir, "demo-link")
        Assume.assumeTrue("requires disposable Host", secret.exists())
        val link = secret.readText().trim()
        secret.delete()
        ui.onNodeWithText("リンクを貼り付ける").performClick()
        ui.onNodeWithContentDescription("ペアリング用のリンク").performTextInput(link)
        press(ui.onNodeWithText("接続"))
        waitFor("スクリーンショットの確認")

        // ---- 画像: 発言・ツール結果・Markdown、省かれたプレビューの取得、全画面 ----
        open("スクリーンショットの確認")
        waitFor(hasContentDescription("修正前の画面"))
        waitFor(hasContentDescription("見つからない画像、読み込めません"))
        waitFor(hasContentDescription("残り 2 枚の画像"))
        // 3 枚目はプレビューが省かれている。entry_image で取りに行って表示する。
        waitFor(hasContentDescription("画像 3 / 5"))
        ui.waitUntil(20000) { !exists(hasContentDescription("画像 3 / 5、読み込めません")) }
        shot("images")
        press(ui.onNodeWithContentDescription("残り 2 枚の画像"))
        waitFor("4 / 5")
        waitFor(hasContentDescription("画像 4 / 5"))
        ui.onNodeWithContentDescription("画像 4 / 5").performTouchInput { swipeLeft() }
        waitFor("5 / 5")
        shot("viewer")
        press(ui.onNodeWithContentDescription("閉じる"))
        gone("5 / 5")

        // ---- / コマンドの候補: 選ぶと入力欄に入るだけ ----
        field.performTextInput("/re")
        waitFor("/review")
        ui.onNodeWithText("Extension").assertExists()
        ui.onNodeWithText("/resume").assertExists()
        shot("commands")
        press(ui.onNodeWithText("/review"))
        ui.onNode(hasSetTextAction() and hasText("/review ")).assertExists()
        field.performTextClearance()

        // ---- モデル・Thinking・お気に入り・既定 ----
        press(ui.onNodeWithContentDescription("モデル: Opus 5.5 · high"))
        waitFor("お気に入り 3")
        shot("sheet-model")
        press(ui.onNodeWithText("すべて 8"))
        press(ui.onNode(hasText("Claude Sonnet 5.5") and hasText("anthropic/claude-sonnet-5-5 · 1M")))
        ui.waitUntil(20000) {
            exists(hasText("Claude Sonnet 5.5") and SemanticsMatcher.expectValue(SemanticsProperties.Selected, true))
        }
        press(ui.onNodeWithText("low"))
        ui.waitUntil(20000) {
            exists(hasText("low") and SemanticsMatcher.expectValue(SemanticsProperties.Selected, true))
        }
        press(ui.onNodeWithContentDescription("GPT-5 をお気に入りに追加"))
        waitFor("お気に入り 4")
        ui.onNodeWithText("保存しました。起動中", substring = true).assertExists()
        press(ui.onNodeWithText("現在のモデルと Thinking を既定にする"))
        waitFor("現在のモデルと Thinking は既定です")
        ui.onNodeWithText("Sonnet 5.5 · low").assertExists()
        back()
        waitFor(hasContentDescription("モデル: Sonnet 5.5 · low"))

        // /model でも同じシートが開き、入力欄は空に戻る。
        field.performTextInput("/model")
        press(ui.onNodeWithContentDescription("送信"))
        waitFor("お気に入り 4")
        back()
        ui.onNode(hasSetTextAction() and hasText("/model")).assertDoesNotExist()

        // ---- 使用状況と圧縮 ----
        press(ui.onNode(hasContentDescription("使用状況", substring = true)))
        waitFor("82K / 200K · 41%")
        ui.onNodeWithText("週間 · 67% 使用").assertDoesNotExist() // この Pi は利用枠をまだ見ていない
        ui.onAllNodesWithText("不明", substring = true).assertCountEquals(2)
        shot("sheet-usage")
        press(ui.onNodeWithText("圧縮する"))
        waitFor("21K / 200K · 11%")
        back()
        waitFor("コンテキストを圧縮しました")
        press(ui.onNodeWithContentDescription("案内を閉じる"))
        gone("コンテキストを圧縮しました")

        // ---- セッション: 分岐（選んだ文が入力欄に入る）・ツリー（ラベル・移動）・新規・再開 ----
        press(ui.onNodeWithContentDescription("セッションのメニュー"))
        waitFor("この Pi を終了…")
        shot("sheet-session")
        ui.onNodeWithText("この Pi は PC のターミナルで直接動いています。").assertExists()
        press(ui.onNodeWithText("ここから分岐"))
        waitFor("どのメッセージから分岐しますか")
        press(
            ui.onNode(
                hasText("残りの 5 枚も確認して") and
                    SemanticsMatcher.keyIsDefined(SemanticsProperties.Selected)
            )
        )
        waitFor("ここから分岐しました")
        waitFor(hasSetTextAction() and hasText("残りの 5 枚も確認して"))
        field.performTextClearance()

        menu("セッションツリー")
        waitFor("別の案: 設定ファイルで切り替える")
        shot("tree")
        press(ui.onNodeWithText("別の案: 設定ファイルで切り替える"))
        waitFor("メッセージの操作")
        ui.onNode(hasSetTextAction() and hasContentDescription("ラベル")).performTextInput("別案")
        press(ui.onNodeWithText("ラベルを保存"))
        waitFor("ラベルを外す")
        shot("tree-node")
        press(ui.onNode(hasText("ここへ移動") and hasClickAction()))
        waitFor("ツリーの位置へ移動しました")
        // 移動先の文は、空の入力欄に入る。
        waitFor(hasSetTextAction() and hasText("別の案: 設定ファイルで切り替える"))
        field.performTextClearance()

        menu("新しいセッション")
        waitFor("新しいセッションを開きました")
        menu("過去のセッションを開く")
        waitFor("過去のセッション")
        waitFor("スクリーンショットの確認")
        press(ui.onNode(hasText("スクリーンショットの確認") and hasText("件", substring = true)))
        waitFor("セッションを開きました")
        waitFor(hasContentDescription("修正前の画面"))

        // ---- PC でしか操作できない画面（Chat-Attach）とコマンドのコピー ----
        open("ブランチの整理")
        waitFor("PC のターミナルで操作が必要です")
        ui.onNodeWithText("PC で操作待ち").assertExists()
        ui.onNodeWithText("pimate attach pimate-0091c2ab").assertExists()
        ui.onNodeWithContentDescription("メッセージ").assertIsNotEnabled()
        shot("chat-attach")
        press(ui.onNodeWithContentDescription("コマンドをコピー"))
        waitFor(hasContentDescription("コピーしました"))
        press(ui.onNodeWithContentDescription("セッションのメニュー"))
        waitFor("スマホ · PC から未接続")
        back()

        // ---- PC での /resume（Chat-Edge）: 下書きは元の会話に残り、送信先を確かめるまで送れない ----
        open("API クライアントのリファクタ")
        field.performTextInput("型エラーも合わせて直して")
        waitFor("PC のターミナルで /resume されました")
        waitFor("送信先が変わりました")
        ui.onNode(hasSetTextAction() and hasText("型エラーも合わせて直して")).assertDoesNotExist()
        shot("chat-edge")
        field.performTextInput("続き")
        ui.onNodeWithContentDescription("送信").assertIsNotEnabled()
        press(ui.onNodeWithText("確認する"))
        gone("送信先が変わりました")
        ui.onNodeWithContentDescription("送信").assertIsEnabled()
        field.performTextClearance()

        // ---- Pi の終了 ----
        open("README の整理")
        menu("この Pi を終了…")
        waitFor("この Pi を終了しますか？")
        press(ui.onNodeWithText("終了する"))
        waitFor("この Pi は終了しました")
    }
}
