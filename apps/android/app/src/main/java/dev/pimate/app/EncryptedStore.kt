package dev.pimate.app

import android.content.Context
import android.security.keystore.*
import android.util.Base64
import java.security.KeyStore
import javax.crypto.*
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject
import uniffi.pimate_ffi.*

class EncryptedStore(context: Context) : SecretStore {
    private val prefs = context.getSharedPreferences("hosts", Context.MODE_PRIVATE)
    private val alias = "pimate.hosts.v1"
    private val key by lazy {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        store.getKey(alias, null) as? SecretKey
            ?: KeyGenerator.getInstance("AES", "AndroidKeyStore").run {
                init(
                    KeyGenParameterSpec.Builder(
                            alias,
                            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                        )
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .build()
                )
                generateKey()
            }
    }

    @Synchronized
    override fun save(host: Host, token: String) {
        val data =
            JSONObject()
                .put("name", host.name)
                .put("address", host.address)
                .put("device", host.deviceId)
                .put("invalid", host.needsPairing)
                .put("token", token)
                .toString()
                .toByteArray()
        val cipher =
            Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(Cipher.ENCRYPT_MODE, key)
                updateAAD(host.id.toByteArray())
            }
        check(
            prefs
                .edit()
                .putString(
                    host.id,
                    Base64.encodeToString(cipher.iv + cipher.doFinal(data), Base64.NO_WRAP),
                )
                .commit()
        )
    }

    @Synchronized
    override fun load(id: String): Credential {
        val bytes =
            Base64.decode(
                prefs.getString(id, null) ?: throw CallbackException.Failed(),
                Base64.NO_WRAP,
            )
        val cipher =
            Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, bytes, 0, 12))
                updateAAD(id.toByteArray())
            }
        val obj = JSONObject(cipher.doFinal(bytes, 12, bytes.size - 12).decodeToString())
        return Credential(
            Host(
                id,
                obj.getString("name"),
                obj.getString("address"),
                obj.getString("device"),
                obj.getBoolean("invalid"),
            ),
            obj.getString("token"),
        )
    }

    @Synchronized override fun hosts() = prefs.all.keys.map { load(it).host }

    @Synchronized
    override fun invalidate(id: String, deviceId: String) {
        if (prefs.contains(id))
            load(id)
                .host
                .takeIf { it.deviceId == deviceId }
                ?.let { save(it.copy(needsPairing = true), "") }
    }

    @Synchronized
    override fun forget(id: String) {
        check(prefs.edit().remove(id).commit())
    }
}
