package dev.pimate.app

import android.app.Application
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.*
import androidx.lifecycle.*
import java.util.concurrent.Semaphore
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.*
import org.json.*
import uniffi.pimate_ffi.*
import uniffi.pimate_ffi.Target

data class Rows(val revision: ULong = 0u, val groups: Map<String, List<Item>> = emptyMap()) {
    internal var notices = ChatNotices()
        private set

    internal var connectionGeneration: Any = Any()
        private set

    private fun advance(next: Rows, changes: List<Change>?): Rows {
        // A resync can skip a whole reconnect. Never reuse a login hold across that gap.
        val connectionChanged =
            changes?.any {
                when (it) {
                    is Change.Upsert -> it.v1.collection == "host" && it.v1.id == "connection"
                    is Change.Remove -> it.collection == "host" && it.id == "connection"
                    else -> false
                }
            } ?: (next.revision != revision)
        if (!connectionChanged) next.connectionGeneration = connectionGeneration
        next.notices = notices.advance(this, next, changes)
        return next
    }

    internal fun snapshotRows(s: Snapshot) = advance(from(s), null)

    fun apply(rev: ULong, changes: List<Change>, snapshot: () -> Snapshot): Rows {
        if (rev <= revision) return this
        // A history subscription may contain thousands of upserts in a single revision.
        if (changes.size > 64 || Change.ResyncRequired in changes) {
            val next = from(snapshot())
            val ordered = changes.takeIf { next.revision == rev && Change.ResyncRequired !in it }
            return advance(next, ordered)
        }
        val next = groups.toMutableMap()
        for (change in changes) {
            when (change) {
                is Change.Upsert ->
                    change.v1.let { item ->
                        val list =
                            next[item.collection]
                                .orEmpty()
                                .filterNot { it.id == item.id }
                                .toMutableList()
                        list.add(item.position.toInt().coerceIn(0, list.size), item)
                        next[item.collection] = list
                    }
                is Change.Remove ->
                    next[change.collection] =
                        next[change.collection].orEmpty().filterNot { it.id == change.id }
                else -> error("unexpected resync")
            }
        }
        return advance(Rows(rev, next), changes)
    }

    fun readyTarget(selected: String?): Target? {
        val view = groups["view"].orEmpty()
        if (selected == null || view.none { it.id == "phase" && it.json == "\"ready\"" })
            return null
        val instance = view.find { it.id == "instanceId" } ?: return null
        if (JSONTokener(instance.json).nextValue() != selected) return null
        val session =
            view
                .find { it.id == "session" }
                ?.json
                ?.takeUnless { it == "null" }
                ?.let { JSONObject(it).getString("id") }
        return Target(selected, session)
    }

    companion object {
        fun from(s: Snapshot) =
            Rows(
                s.revision,
                s.items.groupBy { it.collection }.mapValues { (_, v) -> v.sortedBy { it.position } },
            )
    }
}

// Display and revision checks share the same baseline, including synchronous selection snapshots.
internal class HostRows(private val states: MutableMap<String, Rows>) {
    operator fun get(id: String) = states[id] ?: Rows()

    fun remove(id: String) {
        states.remove(id)
    }

    fun snapshot(id: String, value: Snapshot) {
        states[id] = get(id).snapshotRows(value)
    }

    fun changed(id: String, revision: ULong, changes: List<Change>, snapshot: () -> Snapshot) {
        states[id] = get(id).apply(revision, changes, snapshot)
    }
}

internal enum class Removal {
    REMOVED,
    LOCAL_ONLY,
    UNKNOWN,
}

internal suspend fun revokeMachine(
    supported: Boolean,
    revoke: suspend () -> Boolean,
    removed: () -> Unit,
): Removal {
    if (!supported) return Removal.LOCAL_ONLY
    val revoked =
        try {
            revoke()
        } catch (_: CoreException.OutcomeUnknown) {
            return Removal.UNKNOWN
        }
    if (!revoked) return Removal.LOCAL_ONLY
    removed()
    return Removal.REMOVED
}

internal fun terminalCommand(session: String): String? =
    session
        .takeIf { it.matches(Regex("[A-Za-z0-9][A-Za-z0-9_.-]{0,99}")) }
        ?.let { "pimate attach $it" }

class SessionModel(app: Application, private val saved: SavedStateHandle) : AndroidViewModel(app) {
    internal val settings = AppSettings(app.getSharedPreferences("display", 0))
    var pairing by mutableStateOf(saved.get<Boolean>("additionalPair") == true)
        private set

    fun showPairing(active: Boolean) {
        pairing = active
        saved["additionalPair"] = active
    }

    var pairedHostId by mutableStateOf<String?>(null)
        private set

    fun consumePairedHost() {
        pairedHostId = null
    }

    private val core = Core((app as PiApplication).store)
    private val handler = Handler(Looper.getMainLooper())

    private class Slot {
        var connection: Connection? = null
    }

    private val slots = mutableMapOf<String, Slot>()
    private val states = HostRows(mutableStateMapOf())

    fun rowsFor(id: String) = states[id]

    private val connection
        get() = host?.let { slots[it.id]?.connection }

    var host by mutableStateOf<Host?>(null)
        private set

    var hosts by mutableStateOf(emptyList<Host>())
        private set

    internal val edits = ChatEdits()
    internal val answers = AnswerEdits()
    internal val instructions = ChildInstructions()
    var target by mutableStateOf<Target?>(null)
        private set

    var openList by mutableStateOf(false)
        private set

    fun listed() {
        openList = false
    }

    val rows
        get() = host?.let { states[it.id] } ?: Rows()

    private fun publish() {
        val next = rows
        target = next.readyTarget(selected)
        target?.let { t ->
            host?.let {
                val key = EditorKey(it.id, t)
                if (key != edits.current) error = ""
                edits.current = key
                answers.sync(key, next.groups["question"].orEmpty())
            }
        }
    }

    fun can(op: Operation?): Boolean =
        op != null &&
            !busy &&
            connection?.let {
                try {
                    it.check(op)
                    true
                } catch (_: CoreException) {
                    false
                }
            } == true

    internal fun send(key: EditorKey, body: RestoredDraft) {
        if (key != edits.current || key.target != target || body != edits.body) return
        operate(Operation.Send(key.target, body.text, body.images)) { edits.accepted(key, body) }
    }

    internal fun answer(key: EditorKey, id: String, edit: AnswerEdit, params: JSONObject) {
        if (key != edits.current || key.target != target || answers[key, id] !== edit) return
        operate(Operation.Answer(key.target, params.put("requestId", id).toString()))
    }

    internal fun restore(key: EditorKey, id: String) {
        if (
            busy ||
                key != edits.current ||
                key.target != target ||
                edits.body.text.isNotEmpty() ||
                edits.body.images.isNotEmpty()
        )
            return
        try {
            val body = connection?.takeUnconfirmed(key.target, id) ?: return
            edits.update(key, body)
            error = ""
        } catch (_: CoreException) {
            error = "この未確認の送信は復元できません。履歴を確認してください"
        }
    }

    var selected by mutableStateOf<String?>(null)
        private set

    var error by mutableStateOf("")
        private set

    var busy by mutableStateOf(false)
        private set

    init {
        try {
            hosts = core.hosts()
            for (h in hosts.filterNot { it.needsPairing }) {
                try {
                    connect(h)
                } catch (_: CoreException) {
                    error = "接続を開けません"
                }
            }
            hosts.firstOrNull { it.id in slots }?.let(::focus)
            openList = host != null && !pairing
        } catch (_: CoreException) {
            error = "保存済みの接続を開けません"
        }
    }

    private fun snapshot(id: String, c: Connection) {
        states.snapshot(id, c.snapshot())
        if (host?.id == id) publish()
    }

    private fun connect(h: Host) {
        val slot = Slot()
        val c =
            core.connect(
                h.id,
                object : Listener {
                    private val slots = Semaphore(8)
                    private val resync = AtomicBoolean(false)

                    override fun changed(revision: ULong, changes: List<Change>) {
                        // Do not turn Rust's bounded delivery into an unbounded UI queue.
                        if (resync.get()) return
                        if (!slots.tryAcquire()) {
                            if (resync.compareAndSet(false, true))
                                handler.post {
                                    resync.set(false)
                                    if (this@SessionModel.slots[h.id] === slot)
                                        slot.connection?.let { snapshot(h.id, it) }
                                }
                            return
                        }
                        handler.post {
                            try {
                                if (this@SessionModel.slots[h.id] === slot)
                                    slot.connection?.let {
                                        states.changed(h.id, revision, changes, it::snapshot)
                                        if (host?.id == h.id) publish()
                                    }
                            } finally {
                                slots.release()
                            }
                        }
                    }
                },
            )
        slots.put(h.id, slot)?.connection?.close()
        states.remove(h.id)
        slot.connection = c
        snapshot(h.id, c)
    }

    private fun focus(h: Host) {
        if (host?.id != h.id) connection?.subscribe(null)
        host = h
        selected = null
        edits.current = null
        publish()
    }

    fun pair(link: String) {
        if (!busy)
            viewModelScope.launch {
                busy = true
                error = ""
                try {
                    val first = hosts.isEmpty()
                    val paired = core.pair(link, android.os.Build.MODEL)
                    hosts = core.hosts()
                    connect(paired)
                    showPairing(false)
                    pairedHostId = paired.id.takeIf { first }
                    focus(paired)
                    openList = true
                } catch (_: CoreException) {
                    error = "ペアリングに失敗しました。リンクと接続を確認してください"
                } finally {
                    busy = false
                }
            }
    }

    fun switchHost(h: Host) {
        try {
            connect(h)
            if (host?.id == h.id) focus(h)
            error = ""
        } catch (_: CoreException) {
            error = "接続を開けません"
        }
    }

    fun select(h: Host, id: String) {
        try {
            if (h.id != host?.id) focus(h)
            connection?.subscribe(id)
            if (selected != id) edits.current = null
            selected = id
            connection?.let { snapshot(h.id, it) }
            error = ""
        } catch (_: CoreException) {
            error = "選択できません"
        }
    }

    // Pass the key captured when launching PickMultipleVisualMedia; cancellation is an empty list.
    internal suspend fun attach(key: EditorKey, uris: List<android.net.Uri>) {
        if (uris.isEmpty() || edits.current != key || target != key.target) return
        val view = rows.groups["view"].orEmpty().associate { it.id to it.json }
        require(view["promptImages"] == "true")
        val existing = edits.body.images
        val budget = imageBudget(existing, uris.size, view["promptImageLimit"]?.toIntOrNull() ?: 1)
        val resolver = getApplication<Application>().contentResolver
        val photos = withContext(Dispatchers.IO) { uris.map { encodePhoto(resolver, it, budget) } }
        if (edits.current == key && target == key.target && edits.body.images == existing)
            edits.update(key, edits.body.copy(images = existing + photos))
    }

    // Commands are copied to a terminal; never interpolate unchecked peer-provided text.
    fun attachCommand(): String? =
        rows.groups["view"]
            .orEmpty()
            .find { it.id == "abducoSession" }
            ?.let { (JSONTokener(it.json).nextValue() as? String)?.let(::terminalCommand) }

    fun suggestions(text: String) = connection?.commandSuggestions(text).orEmpty()

    fun stats() = connection?.sessionStats()

    fun allowances(nowMs: ULong) = connection?.allowances(nowMs).orEmpty()

    internal suspend fun request(key: EditorKey, method: RcMethod, params: JSONObject): String {
        val c = connection
        if (c == null || edits.current != key || target != key.target)
            throw CoreException.StaleTarget()
        val result = c.perform(Operation.Rc(key.target, method, params.toString()))
        if (connection !== c || edits.current != key || target != key.target)
            throw CoreException.StaleTarget()
        return result
    }

    internal suspend fun github(key: EditorKey, query: JSONObject): JSONObject {
        val c = connection ?: throw CoreException.Offline()
        if (key != edits.current || key.target != target) throw CoreException.StaleTarget()
        val result = c.perform(Operation.Github(key.target.instance, query.toString()))
        if (c !== connection || key != edits.current || key.target != target)
            throw CoreException.StaleTarget()
        return JSONObject(result)
    }

    internal suspend fun machineRequest(id: String, op: Operation): String {
        require(
            op is Operation.Folders ||
                op is Operation.Dirs ||
                op is Operation.Spawn ||
                op is Operation.HostModels ||
                op is Operation.GithubStatus ||
                op is Operation.Devices ||
                op is Operation.Revoke ||
                op is Operation.UpdateCheck ||
                op is Operation.Update ||
                op is Operation.Restart
        )
        val c = slots[id]?.connection ?: throw CoreException.Offline()
        val result = c.perform(op)
        if (slots[id]?.connection !== c) throw CoreException.StaleTarget()
        return result
    }

    internal suspend fun <T> withLogin(id: String, block: suspend (LoginSession) -> T): T {
        val c = slots[id]?.connection ?: throw CoreException.Offline()
        val generation = rowsFor(id).connectionGeneration
        val hold = java.util.UUID.randomUUID().toString()
        var active = true
        fun valid() =
            active && slots[id]?.connection === c && rowsFor(id).connectionGeneration === generation
        try {
            val login = LoginSession(c, c.openLogin(hold), ::valid)
            if (!valid()) throw CoreException.StaleTarget()
            return block(login)
        } finally {
            active = false
            if (slots[id]?.connection === c) c.closeLogin(hold)
        }
    }

    internal fun retryNow(id: String) {
        slots[id]?.connection?.retryNow()
    }

    internal suspend fun revoke(host: Host): Removal =
        viewModelScope
            .async {
                val c = slots[host.id]?.connection
                revokeMachine(
                    c != null && rowsFor(host.id).supportsRevoke(),
                    { core.revokeAndForget(checkNotNull(c), host.id, host.deviceId) },
                ) {
                    removed(host)
                }
            }
            .await()

    internal fun forget(host: Host) {
        core.forgetDevice(host.id, host.deviceId)
        removed(host)
    }

    private fun removed(registration: Host) {
        val id = registration.id
        if (hosts.none { it.id == id && it.deviceId == registration.deviceId }) return
        slots.remove(id)?.connection?.close()
        states.remove(id)
        edits.forget(id)
        answers.forget(id)
        instructions.forget(id)
        if (pairedHostId == id) consumePairedHost()
        hosts = hosts.filterNot { it.id == id }
        if (host?.id == id) {
            host = null
            selected = null
            edits.current = null
            publish()
            hosts.firstOrNull { it.id in slots }?.let(::focus)
            openList = host != null
        }
    }

    internal fun askAbout(key: EditorKey, review: Review) {
        if (key != edits.current || target != key.target) throw CoreException.StaleTarget()
        val text = edits.body.text
        edits.text(key, review.reference + if (text.isBlank()) "" else "\n$text")
    }

    internal fun abort(key: EditorKey?) {
        if (key != null && key == edits.current && key.target == target)
            operate(Operation.Abort(key.target))
    }

    private fun operate(op: Operation, success: () -> Unit = {}) {
        val c = connection ?: return
        val origin = edits.current
        if (!busy)
            viewModelScope.launch {
                busy = true
                error = ""
                try {
                    c.check(op)
                    c.perform(op)
                    success()
                } catch (e: CoreException) {
                    if (edits.current == origin)
                        error =
                            if (e is CoreException.Rejected) "操作は拒否されました"
                            else "操作を確認できません。再送前に履歴を確認してください"
                } finally {
                    busy = false
                }
            }
    }

    override fun onCleared() {
        slots.values.forEach { it.connection?.close() }
        slots.clear()
        core.close()
    }
}

internal class LoginSession(
    private val connection: Connection,
    val target: Target,
    private val valid: () -> Boolean,
) {
    suspend fun request(method: RcMethod, params: JSONObject = JSONObject()): String {
        require(
            method in
                setOf(
                    RcMethod.AUTH_LIST,
                    RcMethod.AUTH_BEGIN,
                    RcMethod.AUTH_RESPOND,
                    RcMethod.AUTH_CANCEL,
                    RcMethod.AUTH_LOGOUT,
                )
        )
        if (!valid()) throw CoreException.StaleTarget()
        return connection.perform(Operation.Rc(target, method, params.toString())).also {
            if (!valid()) throw CoreException.StaleTarget()
        }
    }

    suspend fun followOnce(): Boolean {
        if (!valid()) throw CoreException.StaleTarget()
        return connection.followLoginOnce(target).also {
            if (!valid()) throw CoreException.StaleTarget()
        }
    }
}
