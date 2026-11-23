package dev.pimate.app

import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class RowsTest {
    @Test
    fun photoBudgetIsSharedWithoutCountingBase64Padding() {
        val image = Image("image/jpeg", "YQ==")
        assertEquals((600 * 1024 - 1) / 3, imageBudget(listOf(image), 3, 4))
        assertEquals(512 * 1024, imageBudget(emptyList(), 1, 1))
        assertThrows(IllegalArgumentException::class.java) { imageBudget(listOf(image), 1, 1) }
    }

    @Test
    fun largeHistoryUsesOneSnapshot() {
        val entries = List(4096) { Item("entry", it.toString(), it.toULong(), "{}") }
        var calls = 0
        val rows =
            Rows().apply(1u, entries.map { Change.Upsert(it) }) {
                calls++
                Snapshot(2u, entries)
            }
        assertEquals(1, calls)
        assertEquals(entries, rows.groups["entry"])
        assertEquals(2uL, rows.revision)
    }

    @Test
    fun moveRemoveAndResync() {
        val a = Item("instance", "a", 0u, "{}")
        val b = a.copy(id = "b", position = 1u)
        val rows = Rows.from(Snapshot(1u, listOf(a, b)))
        val moved =
            rows.apply(2u, listOf(Change.Upsert(b.copy(position = 0u)))) {
                error("unexpected snapshot")
            }
        assertEquals(listOf("b", "a"), moved.groups["instance"]!!.map { it.id })
        val fresh =
            moved.apply(3u, listOf(Change.Remove("instance", "a"), Change.ResyncRequired)) {
                Snapshot(5u, listOf(a))
            }
        assertSame(
            fresh,
            fresh.apply(5u, listOf(Change.Remove("instance", "a"))) { error("old revision") },
        )
        assertTrue(
            fresh
                .apply(6u, listOf(Change.Remove("instance", "a"))) { error("unexpected snapshot") }
                .groups["instance"]!!
                .isEmpty()
        )
    }
}
