package dev.pimate.app

import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class RowsTest {
    @Test
    fun selectedSnapshotIsNewerThanQueuedNotifications() {
        val hosts = HostRows(mutableMapOf())
        val question = Item("question", "answered", 0u, "{}")
        hosts.snapshot("A", Snapshot(10u, listOf(question)))
        hosts.snapshot("B", Snapshot(10u, listOf(question)))
        hosts.snapshot("A", Snapshot(12u, emptyList()))
        hosts.changed("A", 11u, listOf(Change.Upsert(question))) { error("no resync needed") }
        assertEquals(12uL, hosts["A"].revision)
        assertNull(hosts["A"].groups["question"])
        assertEquals(listOf(question), hosts["B"].groups["question"])
    }

    @Test
    fun loginHoldChangesEvenIfNoFrameRenderedWhileOffline() {
        val hosts = HostRows(mutableMapOf())
        val connected = Item("host", "connection", 0u, "\"connected\"")
        fun deliver(rev: ULong, item: Item) =
            hosts.changed("pc", rev, listOf(Change.Upsert(item))) { error("unexpected resync") }
        deliver(1u, connected)
        val old = hosts["pc"].connectionGeneration
        deliver(2u, Item("provider", "login-pi", 0u, "[]"))
        assertSame(old, hosts["pc"].connectionGeneration)
        // Browser is in front: no UI reads between the two connection notifications.
        deliver(3u, connected.copy(json = "\"offline\""))
        deliver(4u, connected)
        val reconnected = hosts["pc"].connectionGeneration
        assertNotSame(old, reconnected)
        hosts.changed("pc", 7u, listOf(Change.ResyncRequired)) { Snapshot(7u, listOf(connected)) }
        assertNotSame(reconnected, hosts["pc"].connectionGeneration)
        val resynced = hosts["pc"].connectionGeneration
        hosts.snapshot("pc", Snapshot(7u, listOf(connected)))
        assertSame(resynced, hosts["pc"].connectionGeneration)
        hosts.remove("pc")
        hosts.snapshot("pc", Snapshot(7u, listOf(connected)))
        assertNotSame(resynced, hosts["pc"].connectionGeneration)
    }

    @Test
    fun copiedCommandOnlyContainsASingleSafeArgument() {
        assertEquals("pimate attach pimate-01234567", terminalCommand("pimate-01234567"))
        for (bad in listOf("", "--help", "host;id", "a\nb")) assertNull(terminalCommand(bad))
    }

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
