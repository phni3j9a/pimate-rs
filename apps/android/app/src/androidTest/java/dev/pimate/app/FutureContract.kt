package dev.pimate.app

import androidx.test.platform.app.InstrumentationRegistry
import java.net.InetAddress
import java.net.ServerSocket
import kotlinx.coroutines.*
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import uniffi.pimate_ffi.*

class FutureContract {
    private val store =
        (InstrumentationRegistry.getInstrumentation().targetContext.applicationContext
                as PiApplication)
            .store

    @Test
    fun nativeWakeCanPollAgainBeforeCallbackReturns() = runBlocking {
        Core(store).use { core ->
            ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
                server.soTimeout = 5000
                val link =
                    "pimate://pair?h=127.0.0.1:${server.localPort}&c=AbCdEfGhIjKlMnOpQrStUv&id=h"
                val result =
                    async(Dispatchers.Unconfined) {
                        runCatching { core.pair(link, "test") }.exceptionOrNull()
                    }
                // The first poll suspended. EOF wakes it on Tokio; Unconfined re-polls inline.
                server.accept().close()
                assertTrue(result.await() is CoreException.Offline)
            }
        }
    }

    @Test
    fun awaitOnJavaDispatchers() {
        Core(store).use { core ->
            runBlocking {
                suspend fun checkInvalidLink() {
                    try {
                        core.pair("invalid", "test")
                        fail("invalid link accepted")
                    } catch (_: CoreException.InvalidLink) {}
                }
                checkInvalidLink() // instrumentation thread
                for (dispatcher in listOf(Dispatchers.Default, Dispatchers.IO, Dispatchers.Main)) {
                    withContext(dispatcher) { checkInvalidLink() }
                }
            }
        }
    }
}
