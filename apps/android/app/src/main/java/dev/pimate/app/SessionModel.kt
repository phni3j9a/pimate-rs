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

class SessionModel(app: Application) : AndroidViewModel(app) {
    private val core = Core((app as PiApplication).store)
    private val handler = Handler(Looper.getMainLooper())
    private var connection: Connection? = null
    private var generation = 0
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
            hosts.firstOrNull { !it.needsPairing }?.let(::connect)
        } catch (_: CoreException) {
            error = "保存済みの接続を開けません"
        }
    }

    private fun connect(h: Host) {
        val epoch = ++generation
        connection?.close()
        connection = null
        edits.current = null
        selected = null
        publish(Rows())
        host = null
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
                                    if (generation == epoch)
                                        connection?.let { publish(Rows.from(it.snapshot())) }
                                }
                            return
                        }
                        handler.post {
                            try {
                                if (generation == epoch)
                                    connection?.let {
                                        publish(rows.apply(revision, changes, it::snapshot))
                                    }
                            } finally {
                                slots.release()
                            }
                        }
                    }
                },
            )
        connection = c
        host = h
        publish(Rows.from(c.snapshot()))
        openList = true
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
            error = ""
        } catch (_: CoreException) {
            error = "接続を開けません"
        }
    }

    fun select(id: String) {
        try {
            connection?.subscribe(id)
            if (selected != id) edits.current = null
            selected = id
            connection?.let { publish(Rows.from(it.snapshot())) }
            error = ""
        } catch (_: CoreException) {
            error = "選択できません"
        }
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
        generation++
        connection?.close()
        core.close()
    }
}
