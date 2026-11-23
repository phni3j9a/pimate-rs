package dev.pimate.app

import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class RowsTest {
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
