package dev.pimate.app

import androidx.test.platform.app.InstrumentationRegistry
import java.util.UUID
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.*

class StoreContract {
    @Test
    fun oldDeviceCannotInvalidateNewPairing() {
        val store =
            (InstrumentationRegistry.getInstrumentation().targetContext.applicationContext
                    as PiApplication)
                .store
        val host = Host(UUID.randomUUID().toString(), "fixture", "127.0.0.1:9", "new", false)
        try {
            store.save(host, "fixture-token")
            store.invalidate(host.id, "old")
            assertEquals(Credential(host, "fixture-token"), store.load(host.id))
            store.invalidate(host.id, host.deviceId)
            assertEquals(Credential(host.copy(needsPairing = true), ""), store.load(host.id))
        } finally {
            store.forget(host.id)
        }
    }
}
