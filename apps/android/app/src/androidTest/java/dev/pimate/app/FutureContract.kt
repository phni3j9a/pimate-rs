package dev.pimate.app

import android.os.Process
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
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
    fun nativeCallbackThreadsCanExit() = runBlocking {
        val saveTid = CompletableDeferred<Int>()
        val deliveryTid = CompletableDeferred<Int>()
        val fixture =
            object : SecretStore by store {
                override fun save(host: Host, token: String) {
                    saveTid.complete(Process.myTid())
                }

                override fun load(id: String) =
                    Credential(Host(id, "test", "127.0.0.1:9", "d", false), "token")
            }
        Core(fixture).use { core ->
            ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
                server.soTimeout = 5000
                val link =
                    "pimate://pair?h=127.0.0.1:${server.localPort}&c=AbCdEfGhIjKlMnOpQrStUv&id=h"
                val paired = async(Dispatchers.Unconfined) { core.pair(link, "test") }
                server.accept().use { socket ->
                    socket.soTimeout = 5000
                    val request = socket.getInputStream().bufferedReader()
                    while (!request.readLine().isNullOrEmpty()) {}
                    val body = """{"hostId":"h","hostName":"test","deviceId":"d","token":"token"}"""
                    socket
                        .getOutputStream()
                        .write(
                            "HTTP/1.1 200 OK\r\nContent-Length: ${body.length}\r\n\r\n$body"
                                .toByteArray()
                        )
                }
                paired.await()
            }
            val listener =
                object : Listener {
                    override fun changed(revision: ULong, changes: List<Change>) {
                        deliveryTid.complete(Process.myTid())
                    }
                }
            core.connect("h", listener).use { withTimeout(5000) { deliveryTid.await() } }
        }
        val tids = listOf(saveTid.await(), deliveryTid.await())
        delay(15_000) // Tokio's idle blocking worker retires after 10s.
        for (tid in tids) assertTrue(
            "callback thread $tid still alive",
            !File("/proc/self/task/$tid").exists(),
        )
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
