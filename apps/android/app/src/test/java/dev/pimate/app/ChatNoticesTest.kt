package dev.pimate.app

import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class ChatNoticesTest {
    private fun up(group: String, id: String, json: String) =
        Change.Upsert(Item(group, id, 0u, json))

    private fun Rows.next(vararg changes: Change) =
        apply(revision + 1u, changes.toList()) { error("unexpected snapshot") }

    private fun view(pi: String) = up("view", "instanceId", "\"$pi\"")

    private val busy =
        up("busy", "operation", """{"id":"request","method":"compact","sessionId":"B"}""")
    private val done =
        up(
            "operationResult",
            "latest",
            """{"id":"request","sessionId":"B","event":{"type":"compact_done"}}""",
        )
    private val remove = Change.Remove("busy", "operation")

    private fun move(from: String, to: String) =
        up(
            "notice",
            "sessionChanged",
            """{"reason":"resume","from":{"id":"$from"},"to":{"id":"$to"}}""",
        )

    @Test
    fun coalescedAndSeparateRevisionsDeliverOnlyToTheInitiatingPiOnce() {
        val a = Rows().next(view("A"))
        for (rows in
            listOf(
                a.next(busy, done, remove),
                a.next(busy).next(done, remove),
                a.next(busy).snapshotRows(Snapshot(3u, listOf(done.v1, view("A").v1))),
            )) {
            val delivery = rows.next(view("B")).notices.receipt!!
            assertFalse(delivery.take("request", "B"))
            assertFalse(delivery.take("old", "A"))
            assertTrue(delivery.take("request", "A"))
            assertFalse(delivery.take("request", "A"))
        }
        assertNull(a.next(done).notices.receipt)
    }

    @Test
    fun identicalIdsOnAnotherHostCannotClaimTheReceipt() {
        val hosts = HostRows(mutableMapOf())
        hosts.changed("first", 1u, listOf(view("pi"), busy)) { error("no snapshot") }
        hosts.changed("second", 1u, listOf(view("pi"), done)) { error("no snapshot") }
        assertNull(hosts["second"].notices.receipt)
    }

    @Test
    fun remoteReturnToCompactedSessionIsNotAPhoneMove() {
        val r =
            Rows().next(view("pi"), busy, done, remove).next(move("B", "A")).next(move("A", "B"))
        assertFalse(r.notices.move!!.byPhone)
        val arrival =
            up(
                "busy",
                "operation",
                """{"id":"new","method":"session_op","op":"switch","sessionId":"A","arrival":"B"}""",
            )
        val local = r.next(arrival, move("A", "B"), remove)
        assertTrue(local.notices.move!!.byPhone)
        assertFalse(local.next(move("A", "B")).notices.move!!.byPhone)
    }

    @Test
    fun confirmingTheFirstMoveDoesNotConfirmTheReturnTrip() {
        val key = EditorKey("h", Target("pi", "B"))
        val first = Rows().next(move("A", "B"))
        val confirmed = SwitchId(key, first.notices.move!!)
        val second = first.next(move("B", "A")).next(move("A", "B"))
        assertNotEquals(confirmed, SwitchId(key, second.notices.move!!))
    }
}
