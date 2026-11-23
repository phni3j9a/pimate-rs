package dev.pimate.app

import android.app.Application
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.*
import androidx.lifecycle.*
import java.util.concurrent.Semaphore
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.*
import uniffi.pimate_ffi.*

data class Rows(val revision: ULong = 0u, val groups: Map<String, List<Item>> = emptyMap()) {
    fun apply(rev: ULong, changes: List<Change>, snapshot: () -> Snapshot): Rows {
        if (rev <= revision) return this
        if (Change.ResyncRequired in changes) return from(snapshot())
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

    var rows by mutableStateOf(Rows())
        private set

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
        rows = Rows()
        selected = null
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
                                        connection?.let { rows = Rows.from(it.snapshot()) }
                                }
                            return
                        }
                        handler.post {
                            try {
                                if (generation == epoch)
                                    connection?.let {
                                        rows = rows.apply(revision, changes, it::snapshot)
                                    }
                            } finally {
                                slots.release()
                            }
                        }
                    }
                },
            )
        connection = c
        rows = Rows.from(c.snapshot())
        host = h
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
            selected = id
            connection?.let { rows = Rows.from(it.snapshot()) }
            error = ""
        } catch (_: CoreException) {
            error = "選択できません"
        }
    }

    fun operate(op: Operation, success: () -> Unit = {}) {
        val c = connection ?: return
        if (!busy)
            viewModelScope.launch {
                busy = true
                error = ""
                try {
                    c.check(op)
                    c.perform(op)
                    success()
                } catch (_: CoreException) {
                    error = "操作を確認できません。再送前に履歴を確認してください"
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
