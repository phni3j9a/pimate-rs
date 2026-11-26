package dev.pimate.app

import org.junit.Assert.*
import org.junit.Test

class TranscriptTest {
    @Test
    fun answeringChildPrefixesExistingDraft() {
        val child = org.json.JSONObject().put("agent", "reviewer").put("title", "確認")
        assertEquals("確認（reviewer）の質問への回答: 下書き", childAnswerDraft(child, "下書き"))
    }

    @Test
    fun thinkingLabelsMatchObservedDuration() {
        assertEquals("Thinking…", thinkingLabel(true, null))
        assertEquals("Thought", thinkingLabel(false, 999))
        assertEquals("Thought for 12s", thinkingLabel(false, 12_100))
        assertEquals("Thought for 1m 0s", thinkingLabel(false, 59_500))
    }

    private val read = Call("c1", "read", "src/greet.ts")
    private val bash = Call("c2", "bash", "npm test")
    private val turn =
        listOf(
            Entry("1", "user", "構成を説明して", time = 1_000),
            Entry(
                "2",
                "assistant",
                "調べます",
                thinking = "考える",
                calls = listOf(read, bash),
                final = false,
            ),
            Entry("3", "tool_result", "a\nb\n", callId = "c1", name = "read"),
            Entry("4", "tool_result", "失敗", callId = "c2", name = "bash", isError = true),
            Entry("5", "assistant", "まとめ", time = 19_000),
        )

    @Test
    fun editRowShowsPatchAndLinksToInitiatingTurn() {
        val rows =
            buildRows(
                listOf(
                    Entry("u", "user"),
                    Entry("a", "assistant", calls = listOf(Call("c", "edit", "foo"))),
                    Entry(
                        "r",
                        "tool_result",
                        "success",
                        callId = "c",
                        change = Piece("c", false, "patch", null, null, null),
                    ),
                ),
                emptyList(),
                false,
            )
        assertEquals("u", rows.filterIsInstance<ChatRow.Summary>().single().reviewTurn)
        assertEquals("patch", rows.filterIsInstance<ChatRow.Tool>().single().change?.patch)
    }

    @Test
    fun completedTurnFoldsWorkButKeepsTheAnswer() {
        val rows = buildRows(turn, emptyList(), active = false)
        assertEquals(
            listOf("1", "turn:1", "2:thinking", "2:text", "tool:2:c1", "tool:2:c2", "5:text"),
            rows.map { it.key },
        )
        assertEquals("18秒 作業 · ツール 2 回", (rows[1] as ChatRow.Summary).text)
        // 畳まれるのは作業だけ。依頼・要約行・最後の回答は常に見える。
        assertEquals(
            listOf("1", "turn:1", "5:text"),
            rows.filter { it.fold == null }.map { it.key },
        )
        assertTrue(rows.filter { it.fold != null }.all { it.fold == "turn:1" })
        val (first, second) = rows.filterIsInstance<ChatRow.Tool>()
        assertEquals(ToolState.Done to "2 行", first.state to first.meta)
        assertEquals(ToolState.Failed to "エラー", second.state to second.meta)
        assertEquals("まとめ", (rows.last() as ChatRow.Text).text)
    }

    @Test
    fun turnInProgressStaysOpenAndShowsLiveWork() {
        val rows =
            buildRows(
                turn.take(3),
                listOf(
                    Live("c2", "bash", "npm test", "running", "> vitest", null, false),
                    Live("c3", "grep", "TODO", "running", "", null, false),
                    Live("c4", "read", "child.ts", "running", "", null, nested = true),
                ),
                active = true,
                streamText = "途中",
                streamThinking = "考え中",
            )
        assertTrue(rows.all { it.fold == null })
        assertEquals(
            listOf(
                "1",
                "2:thinking",
                "2:text",
                "tool:2:c1",
                "tool:2:c2",
                "running:c3",
                "live:thinking",
                "live:text",
            ),
            rows.map { it.key },
        )
        val running = rows.filterIsInstance<ChatRow.Tool>().first { it.key == "tool:2:c2" }
        assertEquals(ToolState.Running to "> vitest", running.state to running.output)
    }

    @Test
    fun answerStartsUnderTheSameThinkingRowAndFreezesItsObservedDuration() {
        fun row(text: String) =
            buildRows(
                    emptyList(),
                    emptyList(),
                    true,
                    text,
                    "観測",
                    liveThinkingMs = if (text.isEmpty()) null else 12_000,
                )
                .filterIsInstance<ChatRow.Thinking>()
                .single()
        val before = row("")
        val after = row("答え")
        assertTrue(before.live)
        assertFalse(after.live)
        assertEquals(before.expansionKey, after.expansionKey) // 手動の開閉設定がそのまま優先される。
        assertEquals("Thought for 12s", thinkingLabel(after.live, after.ms))
        val completed =
            buildRows(
                    listOf(Entry("a", "assistant", "答え", thinking = "観測")),
                    emptyList(),
                    false,
                    streamThinking = "次の思考",
                )
                .filterIsInstance<ChatRow.Thinking>()
        assertEquals(after.expansionKey, completed.first().expansionKey)
        assertNotEquals(after.expansionKey, completed.last().expansionKey)
    }

    @Test
    fun waitingDoesNotFoldTheCurrentTurnAndHiddenToolsDoNotChangeVisibleRows() {
        val history = turn.take(2)
        val waiting = buildRows(history, emptyList(), false, waiting = true)
        assertTrue(waiting.none { it is ChatRow.Summary || it.fold != null })
        for (id in listOf("unseen", "c1")) {
            val nested = Live(id, "read", "", "running", "hidden", null, true)
            assertEquals(waiting, buildRows(history, listOf(nested), false, waiting = true))
        }
    }

    @Test
    fun turnWithoutToolsIsNotFoldedAndDuplicateIdsDoNotRepeat() {
        val plain = listOf(turn[0], Entry("2", "assistant", "答え", thinking = "考える"))
        assertTrue(buildRows(plain, emptyList(), false).all { it.fold == null })
        val rows = buildRows(turn + turn, emptyList(), false)
        assertEquals(rows.size, rows.map { it.key }.toSet().size)
    }

    @Test
    fun resultsFollowTheCallPositionWhenIdsRepeatAcrossTurns() {
        val next =
            listOf(
                Entry("6", "user", "もう一度"),
                Entry("7", "assistant", calls = listOf(read), final = false),
                Entry("8", "tool_result", "c\n", callId = "c1", name = "read"),
                Entry("9", "assistant", "完了"),
            )
        val rows =
            buildRows(turn + next, listOf(Live("c1", "read", "", "done", "", 9, false)), false)
        val tools = rows.filterIsInstance<ChatRow.Tool>().associateBy { it.key }
        assertEquals(setOf("tool:2:c1", "tool:2:c2", "tool:7:c1"), tools.keys)
        assertEquals("a\nb\n" to "2 行", tools["tool:2:c1"]!!.let { it.output to it.meta })
        assertEquals("c\n" to "1 行", tools["tool:7:c1"]!!.let { it.output to it.meta })
        // 後のターンの要約も、自分のツールの行を開ける。
        assertEquals("ツール 1 回", (rows.first { it.key == "turn:6" } as ChatRow.Summary).text)
        assertEquals("turn:6", tools["tool:7:c1"]!!.fold)
        // 呼び出しのない結果は、その結果の行として出す。
        val orphan =
            buildRows(
                listOf(Entry("r", "tool_result", "x", callId = "gone", name = "bash")),
                emptyList(),
                false,
            )
        assertEquals(listOf("result:r"), orphan.map { it.key })
    }

    @Test
    fun stoppedConnectionsAskForActionAndPcPromptIsShownWithoutHistory() {
        assertEquals(Stop.Revoked, stopped("\"revoked\""))
        assertTrue(listOf("invalid_token", "auth_failed").all { stopped("\"$it\"")!!.repair })
        assertFalse(stopped("\"protocol_mismatch\"")!!.repair)
        assertTrue(listOf("connected", "connecting", "offline").all { stopped("\"$it\"") == null })
        assertNull(stopped(null))
        val ready = "\"ready\""
        assertNull(notice(true, ready, true, history = false, asking = false, pcPrompt = true))
        assertNull(notice(true, ready, true, history = false, asking = true, pcPrompt = false))
        assertEquals(false, notice(true, ready, true, false, false, false)?.second)
        assertEquals(true, notice(true, "\"instance_gone\"", true, true, false, false)?.second)
    }

    @Test
    fun formatsDurations() {
        assertEquals("12秒", seconds(12_400))
        assertEquals("1分5秒", seconds(65_000))
        assertEquals("1:04", elapsed(64_900))
        assertEquals("1:02:03", elapsed(3_723_000))
    }
}
