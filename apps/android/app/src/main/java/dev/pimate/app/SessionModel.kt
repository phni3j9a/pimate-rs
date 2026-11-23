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
    fun apply(rev: ULong, changes: List<Change>, snapshot: () -> Snapshot): Rows {
        if (rev <= revision) return this
        // A history subscription may contain thousands of upserts in a single revision.
        if (changes.size > 64 || Change.ResyncRequired in changes) return from(snapshot())
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
        return Rows(rev, next)
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

internal fun terminalCommand(session: String): String? =
    session
        .takeIf { it.matches(Regex("[A-Za-z0-9][A-Za-z0-9_.-]{0,99}")) }
        ?.let { "pimate attach $it" }

class SessionModel(app: Application) : AndroidViewModel(app) {
    private val core = Core((app as PiApplication).store)
    private val handler = Handler(Looper.getMainLooper())

    private class Slot {
        var connection: Connection? = null
    }

    private val slots = mutableMapOf<String, Slot>()
    private val states = mutableStateMapOf<String, Rows>()

    fun rowsFor(id: String) = states[id] ?: Rows()

    private val connection
        get() = host?.let { slots[it.id]?.connection }

    var host by mutableStateOf<Host?>(null)
        private set

    var hosts by mutableStateOf(emptyList<Host>())
        private set

    internal val edits = ChatEdits()
    internal val answers = AnswerEdits()
    var target by mutableStateOf<Target?>(null)
        private set

    var openList by mutableStateOf(false)
        private set

    fun listed() {
        openList = false
    }

    var rows by mutableStateOf(Rows())
        private set

    private fun publish(next: Rows) {
        rows = next
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
            openList = host != null
        } catch (_: CoreException) {
            error = "保存済みの接続を開けません"
        }
    }

    private fun connect(h: Host) {
        val slot = Slot()
        fun update(next: Rows) {
            states[h.id] = next
            if (host?.id == h.id) publish(next)
        }
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
                                        slot.connection?.let { update(Rows.from(it.snapshot())) }
                                }
                            return
                        }
                        handler.post {
                            try {
                                if (this@SessionModel.slots[h.id] === slot)
                                    slot.connection?.let {
                                        update(rowsFor(h.id).apply(revision, changes, it::snapshot))
                                    }
                            } finally {
                                slots.release()
                            }
                        }
                    }
                },
            )
        slots.put(h.id, slot)?.connection?.close()
        slot.connection = c
        update(Rows.from(c.snapshot()))
    }

    private fun focus(h: Host) {
        if (host?.id != h.id) connection?.subscribe(null)
        host = h
        selected = null
        edits.current = null
        publish(rowsFor(h.id))
    }

    fun pair(link: String) {
        if (!busy)
            viewModelScope.launch {
                busy = true
                error = ""
                try {
                    val paired = core.pair(link.trim(), android.os.Build.MODEL)
                    hosts = core.hosts()
                    connect(paired)
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
            connection?.let { publish(Rows.from(it.snapshot())) }
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

    fun operate(op: Operation, success: () -> Unit = {}) {
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
