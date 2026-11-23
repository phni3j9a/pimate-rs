package dev.pimate.app

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.xmlpull.v1.XmlPullParser
import uniffi.pimate_ffi.*

class ReviewContract {
    @Test
    fun readySnapshotWithoutSessionHasTarget() {
        val rows =
            Rows(
                1u,
                mapOf(
                    "view" to
                        listOf(
                            Item("view", "phase", 0u, "\"ready\""),
                            Item("view", "instanceId", 1u, "\"demo\""),
                            Item("view", "session", 2u, "null"),
                        )
                ),
            )
        assertEquals(Target("demo", null), rows.readyTarget("demo"))
        assertNull(rows.readyTarget("another"))
        assertNull(Rows().readyTarget("demo"))
    }

    @Test
    fun backupExcludesEncryptedHosts() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val rules = mutableSetOf<String>()
        var section = ""
        context.resources.getXml(R.xml.data_extraction_rules).use { xml ->
            while (xml.next() != XmlPullParser.END_DOCUMENT) if (
                xml.eventType == XmlPullParser.START_TAG
            ) {
                if (xml.name in listOf("cloud-backup", "device-transfer")) section = xml.name
                if (
                    xml.name == "exclude" &&
                        xml.getAttributeValue(null, "domain") == "sharedpref" &&
                        xml.getAttributeValue(null, "path") == "hosts.xml"
                )
                    rules.add(section)
            }
        }
        assertEquals(setOf("cloud-backup", "device-transfer"), rules)
    }
}
