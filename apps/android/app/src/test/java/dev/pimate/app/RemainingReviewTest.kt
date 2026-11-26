package dev.pimate.app

import org.json.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class RemainingReviewTest {
    @Test
    fun finishedChildCannotStopItsRunningSiblings() {
        val r =
            JSONObject(
                """{"id":"run","active":true,"stopAction":"stop","children":[{"id":"one","state":"running","actions":["stop_child"]},{"id":"two","state":"running"}]}"""
            )
        val approved = r.stopTarget("one")!!
        assertEquals("one", approved.child)
        r.getJSONArray("children").getJSONObject(0).put("actions", JSONArray())
        assertNotEquals(approved, r.stopTarget("one"))
        assertNull(r.stopTarget("one")?.child)
        r.getJSONArray("children").getJSONObject(0).put("state", "complete")
        assertNull(r.stopTarget("one"))
        assertEquals("stop", r.stopTarget(null)?.action)
    }

    @Test
    fun resumedRunCanArriveLaterWithNewChildIds() {
        val pending = JSONObject("""{"id":"next","children":[]}""")
        assertNull(resumedChild(listOf(pending), "next", 2))
        pending.put("children", JSONArray("""[{"index":1,"id":"a"},{"index":2,"id":"b"}]"""))
        assertEquals("b", resumedChild(listOf(pending), "next", 2))
        assertEquals("a", resumedChild(listOf(pending), "next", 4))
    }

    @Test
    fun toolRowKeepsTheFirstRunAfterResume() {
        val first = JSONObject("""{"id":"original","toolCallId":"tool"}""")
        val next = JSONObject("""{"id":"next","toolCallId":"tool","resumedFrom":"original"}""")
        assertSame(first, originalRunsByToolCall(listOf(first, next))["tool"])
    }

    @Test
    fun storageUnknownAndRefreshNeededAreNotGenericFailureOrSuccess() {
        assertTrue(
            authResultText(JSONObject("""{"status":"failed","error":"storage_failed"}"""))
                .contains("保存されたか不明")
        )
        assertTrue(
            authResultText(JSONObject("""{"status":"succeeded","refreshRequired":true}"""))
                .contains("/reload")
        )
        assertTrue(
            authResultText(JSONObject("""{"status":"succeeded"}""")).contains("起動中の Pi には /reload")
        )
    }

    @Test
    fun githubMetadataUsesTheHostCountsAndLabels() {
        val pr =
            JSONObject(
                """{"state":"OPEN","reviewDecision":"APPROVED","ci":{"passed":3,"failed":1,"pending":2},"labels":["UI","Android"]}"""
            )
        assertEquals("OPEN · 承認済み · CI 成功 3・失敗 1・待機 2 · UI · Android", githubMetadata(pr, true))
        assertEquals("OPEN · UI · Android", githubMetadata(pr, false))
        assertEquals("OPEN", githubMetadata(JSONObject("""{"state":"OPEN"}"""), true))
        assertTrue(githubRejected(JSONObject("""{"installed":false}""")).contains("インストール"))
        assertTrue(
            githubRejected(JSONObject("""{"installed":true,"loggedIn":false}"""))
                .contains("gh auth login")
        )
        assertTrue(
            githubRejected(JSONObject("""{"installed":true,"loggedIn":true}""")).contains("閲覧権限")
        )
        assertTrue(githubRejected(null).contains("gh の状態"))
    }

    @Test
    fun externalGithubLinksAcceptOnlyAbsoluteWebUrls() {
        for (u in
            listOf("https://github.com/demo/repo/issues/1", "http://example.com/")) assertEquals(
            u,
            webUrl(u),
        )
        for (u in
            listOf(
                "intent://github.com",
                "file:///tmp/test",
                "javascript:alert(1)",
                "https:/invalid",
                "//example.com",
                "https://user@github.com",
            )) assertNull(webUrl(u))
    }

    @Test
    fun githubReferencePreservesImagesAndRejectsAChangedSessionOrEditor() {
        val edits = ChatEdits()
        val at = EditorKey("pc", Target("pi", "s"))
        val other = EditorKey("other", at.target)
        edits.current = at
        val item = JSONObject("""{"title":"fix","number":4,"url":"https://example.com/4"}""")
        val before = RestoredDraft("existing", listOf(Image("image/png", "fixture")))
        edits.update(at, before)
        assertFalse(edits.insertGithub(at, Target("pi", "new"), item))
        assertFalse(edits.insertGithub(other, other.target, item))
        assertEquals(before, edits.body)
        assertTrue(edits.insertGithub(at, at.target, item))
        assertEquals(before.copy(text = "existing\n#4「fix」 https://example.com/4"), edits.body)
    }

    @Test
    fun missingCliIsNotALoginInstruction() {
        // 照会中と失敗を区別する（失敗したら「確認中…」のままにしない）。
        assertEquals("確認中…", githubStatusText(null))
        assertEquals("確認できませんでした", githubStatusText(null, failed = true))
        val status = JSONObject("""{"installed":false,"loggedIn":false}""")
        assertEquals("PC に gh が入っていません", githubStatusText(status))
        status.put("installed", true)
        assertTrue(githubStatusText(status).contains("PC で実行"))
        status.put("loggedIn", true).put("login", "demo")
        assertTrue(githubStatusText(status).contains("demo でログイン中"))
    }
}
