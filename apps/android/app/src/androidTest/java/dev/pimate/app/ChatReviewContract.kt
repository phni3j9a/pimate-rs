package dev.pimate.app

import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import java.util.concurrent.atomic.AtomicInteger
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
