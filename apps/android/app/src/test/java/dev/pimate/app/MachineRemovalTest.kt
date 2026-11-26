package dev.pimate.app

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.CoreException
import uniffi.pimate_ffi.Item

class MachineRemovalTest {
    @Test
    fun onlyAConnectedCapableHostOffersRemoteRemoval() {
        fun rows(connection: String, info: String) =
            Rows(
                groups =
                    mapOf(
                        "host" to
                            listOf(
                                Item("host", "connection", 0u, "\"$connection\""),
                                Item("host", "info", 0u, info),
                            )
                    )
            )
        val capable = """{"pairedDevices":true}"""
        assertFalse(rows("offline", capable).supportsRevoke())
        assertFalse(rows("connected", "{}").supportsRevoke())
        assertTrue(rows("connected", capable).supportsRevoke())
    }

    @Test
    fun onlyAnAcknowledgedRevokeClearsLocalState() = runBlocking {
        var cleaned = 0
        suspend fun attempt(supported: Boolean, revoke: suspend () -> Boolean) =
            revokeMachine(supported, revoke) { cleaned++ }
        assertEquals(Removal.LOCAL_ONLY, attempt(false) { error("offline must not send") })
        assertEquals(Removal.LOCAL_ONLY, attempt(true) { false })
        assertEquals(Removal.UNKNOWN, attempt(true) { throw CoreException.OutcomeUnknown() })
        assertEquals(0, cleaned)
        assertEquals(Removal.REMOVED, attempt(true) { true })
        assertEquals(1, cleaned)
    }
}
