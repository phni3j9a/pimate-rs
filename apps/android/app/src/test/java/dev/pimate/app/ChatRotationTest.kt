package dev.pimate.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.Target

class ChatRotationTest {
    @Test
    fun recreatedScreenSeesUndismissedResultEvenAfterReceiptWasConsumed() {
        val ui = ChatUi() // Activity-scoped ViewModel shared by both compositions
        val receipt = Receipt("r", "pi")
        val latest = JSONObject("""{"id":"r","outcome":"unknown"}""")
        fun render(): ReceivedResult {
            ui.receive("host", "pi", latest, receipt)
            return checkNotNull(ui.resultFor("host", "pi"))
        }
        val first = render()
        assertFalse(receipt.take("r", "pi"))
        assertSame(first, render())
        assertFalse(render().dismissed)
        assertNull(ui.resultFor("host", "another pi"))
        assertNull(ui.resultFor("another host", "pi"))
        first.dismissed = true
        assertTrue(render().dismissed)
        ui.receive("host", "pi", JSONObject("""{"id":"other-request"}"""), null)
        assertNull(ui.resultFor("host", "pi"))
    }

    @Test
    fun pendingForkSurvivesDismissAndRecreationUntilItsEditorReturns() {
        val ui = ChatUi()
        val r = JSONObject("""{"id":"fork","sessionId":"new","forkText":"次の指示"}""")
        val receipt = Receipt("fork", "pi")
        ui.receive("host", "pi", r, receipt) // result arrives while the editor is not ready
        ui.resultFor("host", "pi")!!.dismissed = true
        ui.receive("host", "pi", r, receipt) // recreated composition
        val result = ui.resultFor("host", "pi")!!
        assertNull(result.takeText(EditorKey("other", Target("pi", "new"))))
        assertNull(result.takeText(EditorKey("host", Target("other", "new"))))
        assertNull(result.takeText(EditorKey("host", Target("pi", "old"))))
        val editor = EditorKey("host", Target("pi", "new"))
        assertEquals("次の指示", result.takeText(editor))
        assertTrue(result.dismissed)
        assertNull(ui.resultFor("host", "pi")!!.takeText(editor))
    }
}
