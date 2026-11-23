import uniffi.pimate_ffi.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.runBlocking

private class Store : SecretStore {
    private var credential: Credential? = null
    @Synchronized override fun hosts() = listOfNotNull(credential?.host)
    @Synchronized override fun save(host: Host, token: String) { credential = Credential(host, token) }
    @Synchronized override fun load(id: String) = credential?.takeIf { it.host.id == id } ?: throw CoreException.Storage()
    @Synchronized override fun invalidate(id: String, deviceId: String) {
        credential?.host?.takeIf { it.id == id && it.deviceId == deviceId }?.needsPairing = true
    }
    @Synchronized override fun forget(id: String) { if (credential?.host?.id == id) credential = null }
}

fun main() = runBlocking {
    val updates = Channel<Item>(Channel.UNLIMITED)
    val listener = object : Listener {
        override fun changed(revision: ULong, changes: List<Change>) {
            changes.filterIsInstance<Change.Upsert>().forEach { updates.trySend(it.v1) }
        }
    }
    Core(Store()).use { core ->
        val host = core.pair(readln(), "Kotlin test")
        check(core.hosts() == listOf(host))
        core.connect(host.id, listener).use { c ->
            while (updates.receive().let { it.collection != "instance" || it.id != "stub" }) {}
            c.subscribe("stub")
            while (updates.receive().let { it.collection != "stream" || it.id != "text" || it.json != "\"hello\"" }) {}
        }
    }
    parse("[あ😀](https://example.org)", false).use { check(plainText(it.blocks().single()) == "あ😀") }
    check(utf16Range("あ😀", 3uL, 7uL) == Utf16Range(1uL, 3uL))
}
