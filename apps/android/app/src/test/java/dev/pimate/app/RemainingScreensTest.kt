package dev.pimate.app

import kotlinx.coroutines.runBlocking
import org.json.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class RemainingScreensTest {
    @Test
    fun chatAndReviewUseSameWriteFallbackAndNeverExposeOmittedBodies() {
        for (reason in listOf("", "binary", "too_large")) {
            val entries =
                JSONArray(
                        """[
              {"kind":"user","entryId":"u"},
              {"kind":"assistant","entryId":"a","toolCalls":[{"name":"write","id":"c","args":{"path":"f","content":"abc"}}]},
              {"kind":"tool_result","entryId":"r","toolName":"write","toolCallId":"c","details":{"pimate":{"kind":"write_diff","omitted":"$reason"}}}
            ]"""
                    )
                    .objects()
            val reviewed = review(entries, "").files.single().pieces.single()
            assertEquals(if (reason.isEmpty()) "abc" else null, reviewed.content)
            assertEquals(
                reviewed,
                buildRows(entries.map(::entry), emptyList(), false)
                    .filterIsInstance<ChatRow.Tool>()
                    .single()
                    .change,
            )
        }
    }

    @Test
    fun beginningSummaryReviewsOnlyCallsBeforeTheFirstPrompt() {
        val entries =
            JSONArray(
                    """[
            {"kind":"assistant","entryId":"a","toolCalls":[{"id":"early","name":"edit","args":{"path":"before"}}]},
            {"kind":"user","entryId":"u","text":"次の依頼"},
            {"kind":"tool_result","entryId":"r","toolCallId":"early","toolName":"edit","text":"完了","detailsOmitted":true},
            {"kind":"assistant","entryId":"b","toolCalls":[{"id":"late","name":"write","args":{"path":"after"}}]},
            {"kind":"tool_result","entryId":"s","toolCallId":"late","toolName":"write"}
        ]"""
                )
                .objects()
        val rows = buildRows(entries.map(::entry), emptyList(), false)
        val beginning = rows.filterIsInstance<ChatRow.Summary>().first().reviewTurn
        assertNotNull(beginning)
        assertEquals(beginning, review(entries.take(3), "").latestChangedTurn)
        val scoped = review(entries, "", beginning)
        assertEquals("このターンの変更（before）について：", scoped.reference)
        assertEquals(listOf("before"), scoped.files.map { it.path })
        assertEquals(listOf("before", "after"), review(entries, "").files.map { it.path })
    }

    @Test
    fun combinedPatchFollowsCallOccurrenceAndNeverCopiesOnlyAPrefix() {
        val entries =
            listOf("one", "two").flatMap { id ->
                JSONArray(
                        """[
            {"kind":"user","entryId":"$id"},
            {"kind":"assistant","toolCalls":[{"id":"same","name":"edit","args":{"path":"f"}}]},
            {"kind":"tool_result","toolCallId":"same","details":{"patch":"$id"}}
        ]"""
                    )
                    .objects()
            }
        for (id in listOf("one", "two")) assertEquals(
            "$id\n",
            clipboardPatch(review(entries, "", id).files),
        )
        val files = review(entries, "").files
        assertEquals("one\ntwo\n", clipboardPatch(files))
        val large = files.first().pieces.first().copy(patch = "x".repeat(CLIPBOARD_CHARS / 2))
        assertNull(clipboardPatch(listOf(ReviewFile("f", listOf(large, large)))))
    }

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
    fun onlyAGreaterPublishedVersionIsNewer() {
        assertTrue(newer("0.2.1", "0.2.0"))
        assertTrue(newer("0.10.0", "0.9.0"))
        assertTrue(newer("1.0.0", "0.99.99"))
        // 開発中の版が公開版より先、同じ版、接尾辞つき、空は案内しない。
        assertFalse(newer("0.2.0", "0.2.1"))
        assertFalse(newer("0.2.0", "0.2.0"))
        assertFalse(newer("0.2.1-beta.1", "0.2.0"))
        assertFalse(newer("0.2.1", "dev"))
        assertFalse(newer("", "0.2.0"))
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

    @Test
    fun confirmedInstructionsStayWithTheirHostAndChild() {
        val a = ChildKey(EditorKey("a", uniffi.pimate_ffi.Target("pi", "s")), "old", "child")
        val sent = ChildInstructions()
        sent.accepted(a, "resume", "continue")
        val current = a.copy(run = "new")
        sent.accepted(current, "steer", "continue")
        assertEquals(listOf("steer", "resume"), sent.forChild(current, "old").map { it.action })
        assertTrue(sent.forChild(a.copy(editor = a.editor.copy(host = "b")), null).isEmpty())
        sent.forget("a")
        assertTrue(sent.forChild(a, null).isEmpty())
    }

    @Test
    fun trustUsesTheExactSelectedPathIncludingTrailingSpace() {
        val form = SpawnForm("host", false)
        form.folder("/work/repo ", true)
        val params = JSONObject(form.operation().params)
        assertEquals("/work/repo ", params.getString("cwd"))
        assertTrue(params.getBoolean("trust"))
        form.folder("/work/repo")
        assertFalse(JSONObject(form.operation().params).getBoolean("trust"))
    }

    @Test
    fun hostModelsTriesNextPiWhenNewestIsNotReady() = runBlocking {
        val rows =
            Rows.from(
                Snapshot(
                    1u,
                    listOf(
                        Item("instance", "older", 0u, "{\"updatedAt\":1}"),
                        Item("instance", "newer", 1u, "{\"updatedAt\":2}"),
                    ),
                )
            )
        val calls = mutableListOf<String>()
        val models = JSONObject()
        assertSame(
            models,
            rows.hostModels { id ->
                calls += id
                if (id == "newer") throw CoreException.Blocked()
                models
            },
        )
        assertEquals(listOf("newer", "older"), calls)
    }

    @Test
    fun childLabelHintsHaveGlobalCountAndTextBudgets() {
        val key = ChildKey(EditorKey("a", uniffi.pimate_ffi.Target("pi", "s")), "run", "child")
        val other = key.copy(editor = key.editor.copy(host = "b"))
        val sent = ChildInstructions()
        sent.accepted(key, "steer", "old")
        repeat(128) { sent.accepted(other, "steer", it.toString()) }
        assertTrue(sent.forChild(key, null).isEmpty())
        assertEquals(128, sent.forChild(other, null).size)
        val long = "x".repeat(32768)
        sent.accepted(key, "steer", long)
        sent.accepted(other, "resume", long)
        assertTrue(sent.forChild(key, null).isEmpty())
        assertEquals(listOf(SentInstruction("resume", long)), sent.forChild(other, null))
        sent.accepted(key, "steer", long + long)
        assertTrue(sent.forChild(key, null).isEmpty())
        assertEquals(1, sent.forChild(other, null).size)
    }

    @Test
    fun successfulSpawnDoesNotImplyPromptWasDeliveredOrInstanceAlreadyListed() = runBlocking {
        val result = JSONObject().put("instanceId", "pi")
        assertEquals(SpawnOutcome.NotListed("pi"), spawnOutcome(result) { false })
        assertEquals(SpawnOutcome.Ready("pi"), spawnOutcome(result) { it == "pi" })
        result.put("messageError", "unknown").put("abducoSession", "unsafe;command")
        assertEquals(
            SpawnOutcome.MessageUnconfirmed("pi", null),
            spawnOutcome(result) { error("must not wait") },
        )
    }
}
