package com.dec.privatemusic.player

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Keystore-backed session storage; the plaintext token never reaches SharedPreferences. */
class SessionStore(context: Context) {
    private val preferences = context.getSharedPreferences("private_music_session", Context.MODE_PRIVATE)

    fun saveJson(raw: String) {
        val key = key()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val iv = cipher.iv
        val encrypted = cipher.doFinal(raw.toByteArray(Charsets.UTF_8))
        preferences.edit().putString("session", Base64.encodeToString(iv + encrypted, Base64.NO_WRAP)).apply()
    }

    fun sync() {
        val stored = preferences.getString("session", null) ?: return
        runCatching {
            val data = Base64.decode(stored, Base64.NO_WRAP)
            val key = key()
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, data.copyOfRange(0, 12))
            val raw = cipher.doFinal(data.copyOfRange(12, data.size)).toString(Charsets.UTF_8)
            platformSignIn(rawToSession(raw))
        }
    }

    fun clear() {
        preferences.edit().clear().apply()
        platformSignOut()
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getEntry("private_music_session_key", null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(KeyGenParameterSpec.Builder("private_music_session_key", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .build())
        return generator.generateKey()
    }
}

private fun rawToSession(raw: String): Session {
    val data = org.json.JSONObject(raw)
    return Session(
        access_token = data.getString("access_token"),
        user_id = data.optLong("user_id").takeIf { it != 0L },
        expires_at = data.optLong("expires_at").takeIf { it > 0 },
        api_user_agent = data.getString("api_user_agent"),
        refresh_cookie = data.getString("refresh_cookie"),
        web_user_agent = data.getString("web_user_agent"),
    )
}
