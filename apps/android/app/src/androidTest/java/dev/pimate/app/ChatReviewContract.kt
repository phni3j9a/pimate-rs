package dev.pimate.app

import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.*
import org.junit.Assert.*
import uniffi.pimate_ffi.*
import uniffi.pimate_ffi.Target

class ChatReviewContract {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()
    private val png =
        "iVBORw0KGgoAAAANSUhEUgAAAAoAAAAFCAIAAADzBuo/AAAAEklEQVR4nGP8z4APMOGVpa00AJ0IAQl63qFDAAAAAElFTkSuQmCC"

    @Test
    fun jumpIncludesRowsAddedDuringAnimation() {
        lateinit var list: LazyListState
        var finished by mutableStateOf(false)
        ui.runOnUiThread {
            ui.activity.setContent {
                list = rememberLazyListState()
                var count by remember { mutableIntStateOf(30) }
                LaunchedEffect(Unit) {
                    snapshotFlow { list.isScrollInProgress }.first { it }
                    count += 10
                }
                LaunchedEffect(Unit) {
                    snapshotFlow { list.layoutInfo.totalItemsCount }.first { it == 30 }
                    list.jumpToLatest()
                    finished = true
                }
                LazyColumn(Modifier.height(180.dp), state = list) {
                    items(count) { Spacer(Modifier.height(48.dp)) }
                }
            }
        }
        ui.waitUntil(5000) { finished }
        ui.runOnIdle {
            assertEquals(40, list.layoutInfo.totalItemsCount)
            assertFalse(list.canScrollForward)
        }
    }

    @Test
    fun tailUsesOneHeightUntilManualExpansionAndRepinsAfterJump() {
        var count by mutableIntStateOf(10)
        var rowHeight by mutableStateOf(40.dp)
        val footerPositions = mutableListOf<Float>()
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    ChatTimeline(Modifier.height(180.dp)) { reverse, hold ->
                        fun footer() =
                            item("footer") {
                                Spacer(
                                    Modifier.height(10.dp).onGloballyPositioned {
                                        footerPositions += it.positionInRoot().y
                                    }
                                )
                            }
                        if (reverse) footer()
                        items(count, key = { if (reverse) count - 1 - it else it }) { index ->
                            val n = if (reverse) count - 1 - index else index
                            Spacer(
                                Modifier.fillMaxWidth()
                                    .height(if (n == 9) rowHeight else 40.dp)
                                    .testTag("row:$n")
                                    .clickable {
                                        hold()
                                        rowHeight += 120.dp
                                    }
                            )
                        }
                        if (!reverse) footer()
                    }
                }
            }
        }
        ui.runOnIdle { rowHeight += 20.dp }
        ui.runOnIdle { assertEquals(1, footerPositions.distinct().size) }
        val previous = ui.onNodeWithTag("row:8").getUnclippedBoundsInRoot().top
        ui.onNodeWithTag("row:9").performClick()
        ui.runOnIdle { count++ }
        assertEquals(previous, ui.onNodeWithTag("row:8").getUnclippedBoundsInRoot().top)
        ui.onNodeWithContentDescription("最新の位置へ").performClick()
        ui.onNodeWithTag("row:10").assertIsDisplayed()
        ui.runOnIdle { count++ }
        ui.onNodeWithTag("row:11").assertIsDisplayed()
        ui.onNodeWithContentDescription("最新の位置へ").assertDoesNotExist()

        repeat(2) { ui.onNodeWithTag("chat").performTouchInput { swipeDown() } }
        val history = ui.onNodeWithTag("row:0").getUnclippedBoundsInRoot().top
        ui.runOnIdle { count++ }
        assertEquals(history, ui.onNodeWithTag("row:0").getUnclippedBoundsInRoot().top)
        ui.onNodeWithContentDescription("最新の位置へ").assertIsDisplayed()
    }

    @Test
    fun growingAnswerKeepsTextUnderPausedFinger() {
        var height by mutableStateOf(500.dp)
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    ChatTimeline(Modifier.height(180.dp)) { _, _ ->
                        item("answer") {
                            Column(Modifier.fillMaxWidth().height(height)) {
                                Spacer(Modifier.height(360.dp))
                                Spacer(Modifier.size(10.dp).testTag("text"))
                            }
                        }
                    }
                }
            }
        }
        fun textTop() = ui.onNodeWithTag("text").getUnclippedBoundsInRoot().top
        val initial = textTop()
        val chat = ui.onNodeWithTag("chat")
        chat.performTouchInput {
            down(center)
            moveBy(Offset(0f, 52f))
        }
        val held = textTop()
        assertTrue(held > initial)
        ui.runOnIdle { height += 40.dp }
        assertEquals(held, textTop())
        chat.performTouchInput { moveBy(Offset(0f, 32f)) }
        assertTrue(textTop() > held)
        chat.performTouchInput { up() }
    }

    @Test
    fun returningToTailFollowsNewRowsBeforeFingerLifts() {
        var count by mutableIntStateOf(8)
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    ChatTimeline(Modifier.height(180.dp)) { reverse, _ ->
                        fun bottom() = item("bottom") { Spacer(Modifier.height(10.dp)) }
                        fun row(i: Int) = if (reverse) count - 1 - i else i
                        if (reverse) bottom()
                        items(count, key = ::row) { i ->
                            Spacer(Modifier.fillMaxWidth().height(48.dp).testTag("row:${row(i)}"))
                        }
                        if (!reverse) bottom()
                    }
                }
            }
        }
        val chat = ui.onNodeWithTag("chat")
        fun lastRowBottom() = ui.onNodeWithTag("row:${count - 1}").getUnclippedBoundsInRoot().bottom
        val bottom = lastRowBottom()
        chat.performTouchInput {
            down(center)
            moveBy(Offset(0f, 72f))
        }
        ui.onNodeWithContentDescription("最新の位置へ").assertIsDisplayed()
        val distance = lastRowBottom() - bottom
        chat.performTouchInput { moveBy(Offset(0f, -distance.toPx())) }
        ui.runOnIdle { count++ }
        assertEquals(bottom, lastRowBottom())
        chat.performTouchInput { up() }
        ui.runOnIdle { count++ }
        assertEquals(bottom, lastRowBottom())
    }

    @Test
    fun nonExpandableSummaryStillOpensItsChangeReview() {
        var reviewed: String? = null
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    RowView(
                        ChatRow.Summary("turn:u", "ツール 1 回", "u", "1 ファイル ›", false),
                        false,
                        review = { reviewed = it },
                    ) {
                        error("no rows to expand")
                    }
                }
            }
        }
        ui.onNodeWithText("ツール 1 回").assert(!hasClickAction())
        ui.onNodeWithText("1 ファイル ›").performClick()
        ui.runOnIdle { assertEquals("u", reviewed) }
    }

    @Test
    fun copyAnswerKeepsMarkdownAndWhitespace() {
        val source = "**太字**\n\n```kt\n\tval n = 1\n```\n"
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) { RowView(ChatRow.Text("answer", source), false) {} }
            }
        }
        ui.onNodeWithContentDescription("回答全文をコピー（Markdown）").performClick()
        ui.runOnIdle {
            val clip =
                ui.activity
                    .getSystemService(android.content.ClipboardManager::class.java)
                    .primaryClip
            assertEquals(source, clip?.getItemAt(0)?.text.toString())
        }
    }

    @Test
    fun clipboardFailureExplainsItWithoutReportingSuccess() {
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    val clipboard = androidx.compose.ui.platform.LocalClipboardManager
                    val failing =
                        object :
                            androidx.compose.ui.platform.ClipboardManager by clipboard.current {
                            override fun setText(
                                annotatedString: androidx.compose.ui.text.AnnotatedString
                            ) {
                                throw android.os.TransactionTooLargeException()
                            }
                        }
                    CompositionLocalProvider(clipboard provides failing) {
                        CopyAction("full text", "差分をコピー")
                    }
                }
            }
        }
        ui.onNodeWithContentDescription("差分をコピー").performClick()
        ui.onNodeWithText("コピーできませんでした").assertIsDisplayed()
        ui.onNodeWithContentDescription("コピーしました").assertDoesNotExist()
        ui.onNodeWithText("閉じる").performClick()
        ui.onNodeWithContentDescription("差分をコピー").assertExists()
    }

    @Test
    fun expandingEditShowsBothResultAndPatch() {
        val patch = "@@ -1 +1 @@\n-old\n+new"
        val tool =
            ChatRow.Tool(
                "c",
                "edit",
                "src/message.kt",
                ToolState.Done,
                "編集しました。",
                "+1 −1",
                change = Piece("c", false, patch, parsePatch(patch), null, null),
                truncated = true,
            )
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    var open by remember { mutableStateOf(false) }
                    RowView(tool, open) { open = !open }
                }
            }
        }
        ui.onNodeWithText(tool.output).assertDoesNotExist()
        ui.onNodeWithText("edit").performClick()
        ui.onNodeWithText(tool.output).assertIsDisplayed()
        ui.onNodeWithText("+ new", substring = true).assertIsDisplayed()
        ui.onNodeWithText("出力の末尾は省略されています。").assertIsDisplayed()
        ui.onNodeWithContentDescription("取得済みの出力をコピー").performClick()
        ui.onNodeWithContentDescription("コピーしました").assertExists()
    }

    @Test
    fun showingMoreDiffLinesNotifiesTheChatBeforeGrowing() {
        val source = "@@ -0,0 +1,125 @@\n" + (1..125).joinToString("\n") { "+line $it" }
        val piece = Piece("id", false, source, parsePatch(source), null, null)
        var holds = 0
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    Column(
                        Modifier.height(360.dp).verticalScroll(rememberScrollState(Int.MAX_VALUE))
                    ) {
                        ChangeDiff(piece) { holds++ }
                    }
                }
            }
        }
        val more = ui.onNodeWithText("続きを表示（残り 5 行）")
        ui.waitForIdle()
        more.assertIsDisplayed()
        more.performClick()
        ui.runOnIdle { assertEquals(1, holds) }
        ui.onNodeWithText("line 125", substring = true).assertExists()
    }

    @Test
    fun swipeDownOnlyClosesAtFitScale() {
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    var open by remember { mutableStateOf(true) }
                    if (open)
                        ImageViewer(listOf(Pic.Inline("1", "image/png", png)), 0) { open = false }
                }
            }
        }
        ui.waitUntil(5000) {
            ui.onAllNodesWithContentDescription("画像").fetchSemanticsNodes().isNotEmpty()
        }
        val image = ui.onNodeWithContentDescription("画像")
        image.performTouchInput { doubleClick() }
        image.performTouchInput { swipeDown() }
        image.assertIsDisplayed()
        image.performTouchInput { doubleClick() }
        image.performTouchInput { swipeDown() }
        image.assertDoesNotExist()
    }

    @Test
    fun sharedTextIsReadOnlyForVisibleTiles() {
        val reads = AtomicInteger()
        val shared =
            object : SharedTextInterface {
                override fun value(): String {
                    reads.incrementAndGet()
                    return "data:image/png;base64,$png"
                }
            }
        val gallery = List(100) { Pic.Shared(ImageKind.DATA, shared) }
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    Column(Modifier.height(180.dp).verticalScroll(rememberScrollState())) {
                        gallery.forEach { PicTile(it, "参照", false, {}) }
                    }
                }
            }
        }
        ui.waitUntil(5000) { reads.get() > 0 }
        ui.waitForIdle()
        assertTrue("reads=${reads.get()}", reads.get() in 1..4)
    }

    @Test
    fun imagesInHeadingAndTableOpenInDocumentOrder() {
        var page = -1
        val pics =
            Pics("test", false, true, { _, _ -> error("no file") }) { images, index ->
                assertEquals(2, images.size)
                assertTrue(images.all { it is Pic.Shared })
                page = index
            }
        val source =
            "# ![見出し写真][p]\n\n| 列 |\n| --- |\n| ![セル写真][p] |\n\n[p]: data:image/png;base64,$png"
        ui.runOnUiThread {
            ui.activity.setContent {
                PiTheme(1f) {
                    CompositionLocalProvider(LocalPics provides pics) { Markdown(source) }
                }
            }
        }
        ui.waitUntil(5000) {
            ui.onAllNodesWithContentDescription("見出し写真", substring = true)
                .fetchSemanticsNodes()
                .isNotEmpty()
        }
        ui.onNodeWithContentDescription("見出し写真", substring = true).performClick()
        ui.runOnIdle { assertEquals(0, page) }
        // 画像は独立したタップ領域。表そのものの「全画面で開く」を実行する。
        ui.onNodeWithText("列").performSemanticsAction(SemanticsActions.OnClick)
        ui.onNodeWithContentDescription("表を Markdown でコピー").assertExists()
        ui.onAllNodesWithContentDescription("セル写真", substring = true).onLast().performClick()
        ui.runOnIdle { assertEquals(1, page) }
    }

    @Test
    fun updatedPcFileIsReadAgainForAnotherAnswer() = runBlocking {
        var requests = 0
        val pics =
            Pics(
                "host/pi/session",
                false,
                true,
                { _, _ ->
                    requests++
                    """{"image":{"data":"$png"}}"""
                },
            )
        repeat(2) { assertNotNull(pics.load(Pic.File("shot.png"), true)) }
        assertEquals(2, requests)
        repeat(2) { assertNotNull(pics.load(Pic.File("shot.png", "answer1"), false)) }
        assertEquals(3, requests)
        assertNotNull(pics.load(Pic.File("shot.png", "answer2"), false))
        assertEquals(4, requests)
    }

    @Test
    fun editingDuringBuiltinReceiptDoesNotLoseText() = runBlocking {
        val key = EditorKey("h", Target("pi", "s"))
        val edits = ChatEdits()
        edits.current = key
        edits.text(key, "/compact")
        edits.runBuiltin(key, edits.body) { edits.text(key, "new message") }
        assertEquals("new message", edits.body.text)
        edits.text(key, "/reload")
        edits.runBuiltin(key, edits.body) {}
        assertEquals("", edits.body.text)
    }

    @Test
    fun rejectedAttachmentsExplainDisconnectAndChangedImageList() {
        val key = EditorKey("host", Target("pi", "s"))
        val image = Image("image/png", png)
        assertNotNull(attachmentRejection(key, key, null, emptyList(), emptyList(), 1))
        assertNotNull(attachmentRejection(key, key, key.target, listOf(image), emptyList(), 1))
        assertNull(attachmentRejection(key, key, key.target, emptyList(), listOf(image), 1))
    }
}
