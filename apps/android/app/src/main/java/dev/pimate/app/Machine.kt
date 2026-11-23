package dev.pimate.app

import androidx.compose.runtime.*
import kotlinx.coroutines.flow.first
import org.json.*
import uniffi.pimate_ffi.*

internal enum class FirstStep {
    LOADING,
    OFFLINE,
    EMPTY,
    LIST,
}

internal fun Rows.firstStep(): FirstStep {
    val host = groups["host"].orEmpty().associate { it.id to it.json }
    return when (host["connection"]) {
        "\"connected\"" ->
            if (host["listKnown"] != "true") FirstStep.LOADING
            else if (groups["instance"].isNullOrEmpty()) FirstStep.EMPTY else FirstStep.LIST
        "\"offline\"",
        "\"invalid_token\"",
        "\"auth_failed\"",
        "\"revoked\"",
        "\"protocol_mismatch\"" -> FirstStep.OFFLINE
        else -> FirstStep.LOADING
    }
}

internal suspend fun SessionModel.hostModels(id: String): JSONObject? {
    val recent =
        rowsFor(id).groups["instance"].orEmpty().sortedByDescending {
            JSONObject(it.json).optLong("updatedAt")
        }
    for (pi in recent.take(3)) {
        try {
            return JSONObject(machineRequest(id, Operation.HostModels(pi.id)))
        } catch (_: CoreException.Rejected) {
            continue
        } catch (_: CoreException.Unsupported) {
            continue
        } catch (_: CoreException.StaleTarget) {
            continue
        }
    }
    return null // No Pi yet: allow spawn with the PC default.
}

/** A new composer per machine. Never carry a trust decision to a different directory. */
internal class SpawnForm(val host: String, askDefault: Boolean) {
    var cwd by mutableStateOf("")
        private set

    var trust by mutableStateOf(false)
    var askTool by mutableStateOf(askDefault)
    var message by mutableStateOf("")
    var model by mutableStateOf<JSONObject?>(null)
    var thinking by mutableStateOf<String?>(null)

    fun folder(path: String, knownTrusted: Boolean = false) {
        cwd = path
        trust = knownTrusted
    }

    fun operation(): Operation.Spawn {
        require(cwd.isNotBlank())
        val params =
            JSONObject()
                .put("cwd", cwd.trim())
                .put("trust", trust)
                .put("askTool", askTool)
                .put("model", model)
                .put("thinking", thinking.takeIf { model != null })
                .put("message", message.trim().ifEmpty { null })
        return Operation.Spawn(params.toString())
    }
}

internal sealed interface SpawnOutcome {
    data class Ready(val instance: String) : SpawnOutcome

    data class NotListed(val instance: String) : SpawnOutcome

    data class MessageUnconfirmed(val instance: String, val attach: String?) : SpawnOutcome
}

internal suspend fun spawnOutcome(
    result: JSONObject,
    listed: suspend (String) -> Boolean,
): SpawnOutcome {
    val id = result.getString("instanceId")
    return when {
        result.str("messageError").isNotEmpty() ->
            SpawnOutcome.MessageUnconfirmed(id, terminalCommand(result.str("abducoSession")))
        listed(id) -> SpawnOutcome.Ready(id)
        else -> SpawnOutcome.NotListed(id)
    }
}

internal suspend fun SessionModel.spawn(form: SpawnForm): SpawnOutcome =
    spawnOutcome(JSONObject(machineRequest(form.host, form.operation()))) { id ->
        kotlinx.coroutines.withTimeoutOrNull(20_000) {
            snapshotFlow { rowsFor(form.host).groups["instance"].orEmpty().any { it.id == id } }
                .first { it }
        } ?: false
    }
