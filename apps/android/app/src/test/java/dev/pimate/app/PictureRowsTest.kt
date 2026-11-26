package dev.pimate.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class PictureRowsTest {
    @Test
    fun millionsOfSlotsKeepOnlyTheTwoPreviewsAndTheWholeRangeIsAddressable() {
        val count = 100_000_000
        val pictures =
            entry(
                    JSONObject(
                        """{"entryId":"r","kind":"tool_result","imageCount":$count,"images":[{},{}],"imageIndices":[2,${count - 2}]}"""
                    )
                )
                .pics
        val all = pictures.gallery(true)
        assertEquals(count, all.size)
        assertEquals(Pic.Entry("r", count - 1), all.last())
        assertEquals(listOf(all[2], all[count - 2]), pictures.gallery(false))
    }

    @Test
    fun imageOnlyEditKeepsItsReviewWithoutAnEmptyExpander() {
        val history =
            listOf(
                Entry("u", "user"),
                Entry("a", "assistant", calls = listOf(Call("c", "edit", "foo"))),
                Entry(
                    "r",
                    "tool_result",
                    callId = "c",
                    pics = ImageSlots("r", 1, emptyMap()),
                    change = Piece("c", false, "patch", null, null, null),
                ),
            )
        fun summary(entries: List<Entry>) =
            buildRows(entries, emptyList(), false).filterIsInstance<ChatRow.Summary>().single()
        assertFalse(summary(history).expandable)
        assertEquals("u", summary(history).reviewTurn)
        assertTrue(summary(history + Entry("b", "assistant", thinking = "考える")).expandable)
    }

    @Test
    fun toolImageStaysInCollapsedTurn() {
        val pics = ImageSlots("r", 1, emptyMap())
        val rows =
            buildRows(
                listOf(
                    Entry("u", "user"),
                    Entry("a", "assistant", calls = listOf(Call("t", "shot", "")), final = false),
                    Entry("r", "tool_result", callId = "t", pics = pics),
                    Entry("f", "assistant", text = "done"),
                ),
                emptyList(),
                false,
            )
        val tool = rows.filterIsInstance<ChatRow.Tool>().single()
        assertNull(tool.fold)
        assertEquals(pics, tool.pics)
    }
}
