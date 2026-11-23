package dev.pimate.app

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.xmlpull.v1.XmlPullParser
import uniffi.pimate_ffi.*

class ReviewContract {
    @Test
    fun staleImeCommitCannotCrossEditorKeys() {
        val model = ChatEdits()
        val old = EditorKey("a", Target("demo", "s"))
        val new = EditorKey("a", Target("demo", "next"))
        model.current = old
        model.text(old, "変換中")
        model.current = new
        model.text(old, "旧セッションからの確定")
        assertEquals("", model.body.text)
        model.text(new, "別の入力")
        model.current = old
        assertEquals("変換中", model.body.text)
    }

    @Test
    fun unansweredQuestionsSurviveTargetChangesAndAreReleasedOnResolution() {
        val model = AnswerEdits()
        val key = EditorKey("host", Target("i", "s"))
        val other = key.copy(host = "other")
        val pending = listOf(Item("question", "q", 0u, """{"kind":"editor","message":"初期値"}"""))
        model.sync(key, pending)
        val first = model[key, "q"]!!
        first.value = "書きかけ"
        first.option = "choice"
        first.choices[0] = Choice(setOf("選択"), "補足")
        model.sync(other, pending)
        assertEquals("初期値", model[other, "q"]!!.value)
        model.sync(key, pending)
        assertSame(first, model[key, "q"])
        model.sync(key, emptyList())
        assertNull(model[key, "q"])
        model.sync(key, pending)
        assertEquals("初期値", model[key, "q"]!!.value)
        assertTrue(model[key, "q"]!!.choices.isEmpty())
    }

    @Test
    fun readySnapshotWithoutSessionHasTarget() {
        val rows =
            Rows(
                1u,
                mapOf(
                    "view" to
                        listOf(
                            Item("view", "phase", 0u, "\"ready\""),
                            Item("view", "instanceId", 1u, "\"demo\""),
                            Item("view", "session", 2u, "null"),
                        )
                ),
            )
        assertEquals(Target("demo", null), rows.readyTarget("demo"))
        assertNull(rows.readyTarget("another"))
        assertNull(Rows().readyTarget("demo"))
    }

    @Test
    fun backupExcludesEncryptedHosts() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val rules = mutableSetOf<String>()
        var section = ""
        context.resources.getXml(R.xml.data_extraction_rules).use { xml ->
            while (xml.next() != XmlPullParser.END_DOCUMENT) if (
                xml.eventType == XmlPullParser.START_TAG
            ) {
                if (xml.name in listOf("cloud-backup", "device-transfer")) section = xml.name
                if (
                    xml.name == "exclude" &&
                        xml.getAttributeValue(null, "domain") == "sharedpref" &&
                        xml.getAttributeValue(null, "path") == "hosts.xml"
                )
                    rules.add(section)
            }
        }
        assertEquals(setOf("cloud-backup", "device-transfer"), rules)
    }
}
