package dev.pimate.app

import java.net.URL
import javax.net.ssl.HttpsURLConnection
import kotlinx.coroutines.*
import org.json.JSONArray

internal const val REPO = "https://github.com/phni3j9a/pimate-rs"
internal const val REPORT_ISSUE = "$REPO/issues/new"

internal sealed interface AppUpdate {
    data object Unknown : AppUpdate

    data object Latest : AppUpdate

    data class Available(val version: String, val url: String) : AppUpdate
}

private val versionPattern = Regex("[0-9]+\\.[0-9]+\\.[0-9]+")

/** latest が current より新しい x.y.z のときだけ true。接尾辞つきや壊れた値は新しいと見なさない（TS 版の newer と同じ）。 */
internal fun newer(latest: String, current: String): Boolean {
    if (!latest.matches(versionPattern) || !current.matches(versionPattern)) return false
    val cmp =
        latest
            .split('.')
            .zip(current.split('.'))
            .map { (a, b) ->
                // 桁数、次に文字の順で比べる（数値に直さないので、長い番号でもあふれない）。
                val x = a.trimStart('0')
                val y = b.trimStart('0')
                x.length.compareTo(y.length).takeUnless { it == 0 } ?: x.compareTo(y)
            }
            .firstOrNull { it != 0 } ?: 0
    return cmp > 0
}

internal fun releaseStatus(current: String, releases: JSONArray): AppUpdate {
    if (!current.matches(versionPattern)) return AppUpdate.Unknown
    for (r in releases.objects()) {
        if (r.opt("draft") != false || r.opt("prerelease") != false) continue
        val tag = r.str("tag_name")
        if (!tag.startsWith("app-v")) continue
        val version = tag.removePrefix("app-v")
        if (!version.matches(versionPattern)) continue
        return if (newer(version, current)) AppUpdate.Available(version, "$REPO/releases/tag/$tag")
        else AppUpdate.Latest
    }
    return AppUpdate.Unknown
}

internal suspend fun checkAppUpdate(current: String): AppUpdate =
    withContext(Dispatchers.IO) {
        val c =
            URL("https://api.github.com/repos/phni3j9a/pimate-rs/releases?per_page=100")
                .openConnection() as HttpsURLConnection
        try {
            c.instanceFollowRedirects = false
            c.readTimeout = 10000
            c.connectTimeout = 10000
            if (c.responseCode != 200) AppUpdate.Unknown
            else {
                val body =
                    c.inputStream.bufferedReader().use { reader ->
                        val buffer = CharArray(8192)
                        val result = StringBuilder()
                        while (result.length <= 2_000_000) {
                            val n = reader.read(buffer)
                            if (n < 0) break
                            result.append(buffer, 0, n)
                        }
                        result
                    }
                if (body.length > 2_000_000) AppUpdate.Unknown
                else releaseStatus(current, JSONArray(body.toString()))
            }
        } catch (_: java.io.IOException) {
            AppUpdate.Unknown
        } catch (_: org.json.JSONException) {
            AppUpdate.Unknown
        } finally {
            c.disconnect()
        }
    }
