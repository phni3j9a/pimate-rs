import uniffi.pimate_ffi.*
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

private class Store : SecretStore {
    @Volatile private var credential: Credential? = null
    @Volatile var corruptToken = false
    val entered = CountDownLatch(2)
    val release = CountDownLatch(1)
    private fun slowFailure(): Nothing {
        entered.countDown()
        check(release.await(10, TimeUnit.SECONDS))
        throw java.io.IOException("secret detail must not cross FFI")
    }
    override fun hosts() = listOfNotNull(credential?.host)
    override fun save(host: Host, token: String) {
        if (credential != null) slowFailure()
        credential = Credential(host, token)
    }
    override fun load(id: String): Credential {
        val saved = credential?.takeIf { it.host.id == id } ?: throw CallbackException.Failed()
        return if (corruptToken) saved.copy(token = "invalid") else saved
    }
    override fun invalidate(id: String, deviceId: String) { slowFailure() }
    override fun forget(id: String) { error("unexpected storage error") }
}

fun main() = runBlocking {
    val batches = Channel<List<Change>>(Channel.UNLIMITED)
    val listener = object : Listener {
        var first = true
        override fun changed(revision: ULong, changes: List<Change>) {
            if (first) { first = false; throw AssertionError("lost batch") }
            batches.trySend(changes)
        }
    }
    val store = Store()
    Core(store).use { core ->
        val host = core.pair(readln(), "Kotlin test")
        val secondLink = readln()
        check(core.hosts() == listOf(host))
        core.connect(host.id, listener).use { c ->
            check(batches.receive() == listOf(Change.ResyncRequired))
            val snapshot = c.snapshot()
            if (snapshot.items.none { it.collection == "instance" && it.id == "stub" }) {
                while (batches.receive().filterIsInstance<Change.Upsert>().none { it.v1.collection == "instance" && it.v1.id == "stub" }) {}
            }
            store.corruptToken = true
            val invalidations = Channel<Item>(Channel.UNLIMITED)
            val invalid = object : Listener {
                override fun changed(revision: ULong, changes: List<Change>) { changes.filterIsInstance<Change.Upsert>().forEach { invalidations.trySend(it.v1) } }
            }
            core.connect(host.id, invalid).use {
                val save = async(Dispatchers.Default) { runCatching { core.pair(secondLink, "slow save") }.exceptionOrNull() }
                try {
                    check(store.entered.await(5, TimeUnit.SECONDS))
                    // Both store calls are blocked. A network operation must still complete.
                    withTimeout(3000) { c.perform(Operation.Devices) }
                } finally { store.release.countDown() }
                check(save.await() is CoreException.Storage)
                while (invalidations.receive().let { it.id != "storageError" || it.json != "true" }) {}
                while (invalidations.receive().let { it.id != "connection" || it.json != "\"invalid_token\"" }) {}
            }
            c.subscribe("stub")
            while (batches.receive().filterIsInstance<Change.Upsert>().none { it.v1.collection == "stream" && it.v1.json == "\"hello\"" }) {}
        }
        check(runCatching { core.forget(host.id) }.exceptionOrNull() is CoreException.Storage)
    }
    parse("[あ😀](https://example.org)", false).use { check(plainText(it.blocks().single()) == "あ😀") }
    parse("![alt](https://example.org/img.png)", false).use { check(it.images().single().src.value() == "https://example.org/img.png") }
    check(utf16Range("あ😀", 3uL, 7uL) == Utf16Range(1uL, 3uL))
}
