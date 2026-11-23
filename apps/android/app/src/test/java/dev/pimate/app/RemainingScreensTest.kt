package dev.pimate.app

import kotlinx.coroutines.runBlocking
import org.json.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class RemainingScreensTest {
    @Test
    fun reviewScopesCallsButFindsLateResultsAndNeverCountsUnknownContent() {
        val rows =
            JSONArray(
                    """[
        {"kind":"user","entryId":"first"},
        {"kind":"assistant","toolCalls":[{"id":"a","name":"edit","args":{"path":"/repo/a"}}]},
        {"kind":"user","entryId":"second"},
        {"kind":"assistant","toolCalls":[{"id":"b","name":"write","args":{"path":"x/../a","content":"abc"}}]},
        {"kind":"tool_result","toolCallId":"b"},
        {"kind":"tool_result","toolCallId":"a","details":{"patch":"--- a\n+++ a\n@@ -1 +1 @@\n--- old\n+++ new\n--- b\n+++ b\n@@ -0,0 +4 @@\n+x\n\\ No newline at end of file"}}]"""
                )
                .objects()
        val first = review(rows, "/repo", "first")
        assertEquals("このターンの変更（a）について：", first.reference)
        val patch = first.files.single().pieces.single().diff!!
        assertEquals(2, patch.add)
        assertEquals(1, patch.del)
        assertEquals(4L, patch.hunks.last().lines.single().new)
        assertTrue(patch.hunks.last().lines.single().noNewline)
        val pieces = review(rows, "/repo").files.single().pieces
        assertEquals(listOf("a", "b"), pieces.map { it.call })
        assertNull(pieces.last().diff)
        assertEquals("abc", pieces.last().content)
        assertTrue(review(rows, "/repo", "missing").files.isEmpty())
    }

    @Test
    fun childPagesAreBoundedAndMergedById() = runBlocking {
        var calls = 0
        val record = WorkRecord { cursor ->
            if (calls == 0) assertNull(cursor) else assertEquals("opaque:$calls", cursor)
            calls++
            JSONObject()
                .put("cursor", "opaque:${calls.coerceAtMost(8)}")
                .put("items", JSONArray().put(JSONObject().put("entryId", "a").put("text", calls)))
                .put("truncated", calls == 1)
        }
        record.load()
        assertEquals(8, calls)
        assertTrue(record.more)
        assertTrue(record.truncated)
        record.load()
        assertFalse(record.more)
        assertEquals(9, record.items.single().getInt("text"))
        val run = Item("run", "r", 0u, """{"children":[{"state":"waiting"},{"state":"queued"}]}""")
        val watch = Item("runWatch", "r", 0u, "true")
        assertEquals(
            mapOf("waiting" to 1),
            Rows.from(Snapshot(1u, listOf(run, watch))).subagentCounts(),
        )
    }

    @Test
    fun neverShowEmptyUntilHostSentAList() {
        fun phase(state: String, known: Boolean, list: Boolean = false): FirstStep {
            val items =
                mutableListOf(
                    Item("host", "connection", 0u, JSONObject.quote(state)),
                    Item("host", "listKnown", 1u, known.toString()),
                )
            if (list) items += Item("instance", "i", 0u, "{}")
            return Rows.from(Snapshot(1u, items)).firstStep()
        }
        assertEquals(FirstStep.LOADING, phase("connected", false))
        assertEquals(FirstStep.EMPTY, phase("connected", true))
        assertEquals(FirstStep.OFFLINE, phase("revoked", true))
        assertEquals(FirstStep.LIST, phase("connected", true, true))
    }

    @Test
    fun versionCheckOnlyOffersThisAppsRelease() {
        val list =
            JSONArray(
                """[{"draft":false,"prerelease":true,"tag_name":"app-v2.0.0"},{"draft":false,"prerelease":false,"tag_name":"app-v0.10.0","html_url":"https://example.org"}]"""
            )
        assertEquals(
            AppUpdate.Available("0.10.0", "$REPO/releases/tag/app-v0.10.0"),
            releaseStatus("0.9.0", list),
        )
        assertEquals(AppUpdate.Latest, releaseStatus("1.0.0", list))
        assertEquals(AppUpdate.Unknown, releaseStatus("0.1.0", JSONArray()))
    }

    @Test
    fun spawnNeverReusesTrustForAnotherPath() {
        val form = SpawnForm("hostB", true)
        form.folder("/repo", true)
        form.folder("/new")
        form.message = " first "
        form.thinking = "high"
        val p = JSONObject(form.operation().params)
        assertFalse(p.getBoolean("trust"))
        assertTrue(p.getBoolean("askTool"))
        assertFalse(p.has("thinking"))
        assertEquals("first", p.getString("message"))
        assertEquals("/new", p.getString("cwd"))
    }
}
