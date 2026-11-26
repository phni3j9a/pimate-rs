package dev.pimate.app

import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.pimate_ffi.Operation

class MachineJourney {
    @Test
    fun reviewAndHostToolsWithoutChangingTheSelection() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val file = File(context.cacheDir, "machine-link")
        assumeTrue("requires disposable demo Host", file.exists())
        val link = file.readText().trim()
        file.delete()
        require(uniffi.pimate_ffi.previewPair(link)?.address?.startsWith("127.0.0.1:") == true)
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var m: SessionModel
            scenario.onActivity { m = ViewModelProvider(it)[SessionModel::class.java] }
            runBlocking {
                withContext(Dispatchers.Main) {
                    withTimeout(20000) {
                        assumeTrue("requires empty RS test registration", m.hosts.isEmpty())
                        m.pair(link)
                        val id = snapshotFlow { m.pairedHostId }.first { it != null }!!
                        try {
                            snapshotFlow { m.rowsFor(id).firstStep() }
                                .first { it == FirstStep.LIST }
                            assertNull(m.selected)
                            assertTrue(m.hostModels(id)!!.getJSONArray("models").length() > 0)
                            val devices =
                                JSONObject(m.machineRequest(id, Operation.Devices))
                                    .getJSONArray("devices")
                            assertTrue(devices.objects().single().getBoolean("current"))
                            assertNull(m.selected)
                            m.select(m.hosts.single(), "main")
                            snapshotFlow { m.target }.first { it?.instance == "main" }
                            val key = m.edits.current!!
                            val r =
                                review(
                                    m.rows.groups.getValue("entry").map { JSONObject(it.json) },
                                    "/home/me/code/demo",
                                )
                            assertEquals(2, r.files.size)
                            assertNotNull(r.latestChangedTurn)
                            m.edits.text(key, "ここを確認")
                            m.askAbout(key, r)
                            assertEquals(r.reference + "\nここを確認", m.edits.body.text)
                            assertTrue(m.rows.groups["draft"].isNullOrEmpty())
                            m.consumePairedHost()
                            assertNull(m.pairedHostId)
                        } finally {
                            m.forget(m.hosts.single { it.id == id })
                        }
                    }
                }
            }
        }
    }
}
