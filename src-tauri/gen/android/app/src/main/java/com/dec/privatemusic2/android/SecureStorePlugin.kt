package com.dec.privatemusic2.android

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileOutputStream
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
internal class SetArgs {
  lateinit var value: String
}

/**
 * The signed-in session, encrypted at rest.
 *
 * The key is an AES-256-GCM key created inside the Android Keystore (it never
 * leaves it); the ciphertext is the only thing written to disk, in the app's
 * private files directory. The text is never logged. Only Rust calls this
 * plugin (it exposes no JavaScript permissions).
 */
@TauriPlugin
class SecureStorePlugin(activity: Activity) : Plugin(activity) {
  private val vault = SessionVault(activity.applicationContext.filesDir)

  @Command
  fun get(invoke: Invoke) {
    try {
      val result = JSObject()
      vault.read()?.let { result.put("value", it) }
      invoke.resolve(result)
    } catch (e: Exception) {
      invoke.reject("secure store read failed: ${e.javaClass.simpleName}")
    }
  }

  @Command
  fun set(invoke: Invoke) {
    try {
      vault.write(invoke.parseArgs(SetArgs::class.java).value)
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject("secure store write failed: ${e.javaClass.simpleName}")
    }
  }

  @Command
  fun delete(invoke: Invoke) {
    try {
      vault.clear()
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject("secure store delete failed: ${e.javaClass.simpleName}")
    }
  }
}

internal class SessionVault(filesDir: File) {
  private val dir = File(filesDir, "secure")
  private val file = File(dir, "session.bin")

  @Synchronized
  fun write(text: String) {
    dir.mkdirs()
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, key())
    cipher.updateAAD(AAD)
    val sealed = cipher.doFinal(text.toByteArray(Charsets.UTF_8))
    val iv = cipher.iv
    val tmp = File(dir, "session.bin.tmp")
    FileOutputStream(tmp).use { out ->
      out.write(VERSION)
      out.write(iv)
      out.write(sealed)
      out.fd.sync()
    }
    Files.move(tmp.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
  }

  /** `null` when nothing is stored, or when it cannot be read any more (the key is gone or the file damaged: treated as signed out). */
  @Synchronized
  fun read(): String? {
    if (!file.exists()) return null
    return try {
      val bytes = file.readBytes()
      require(bytes.size > 1 + IV_LEN + TAG_BYTES && bytes[0] == VERSION.toByte())
      val cipher = Cipher.getInstance(TRANSFORMATION)
      cipher.init(
        Cipher.DECRYPT_MODE,
        key(),
        GCMParameterSpec(TAG_BYTES * 8, bytes.copyOfRange(1, 1 + IV_LEN)),
      )
      cipher.updateAAD(AAD)
      String(cipher.doFinal(bytes.copyOfRange(1 + IV_LEN, bytes.size)), Charsets.UTF_8)
    } catch (e: Exception) {
      Log.w(TAG, "stored session unreadable (${e.javaClass.simpleName}); signing out")
      discard()
      null
    }
  }

  @Synchronized
  fun clear() = discard()

  private fun discard() {
    file.delete()
    File(dir, "session.bin.tmp").delete()
    try {
      keyStore().deleteEntry(ALIAS)
    } catch (_: Exception) {
    }
  }

  private fun keyStore(): KeyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

  private fun key(): SecretKey {
    val store = keyStore()
    (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
      KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build()
    )
    return generator.generateKey()
  }

  private companion object {
    const val TAG = "PrivateMusic"
    const val ALIAS = "pm_session_key_v1"
    const val TRANSFORMATION = "AES/GCM/NoPadding"
    const val VERSION = 1
    const val IV_LEN = 12
    const val TAG_BYTES = 16
    val AAD = "private-music:session:v1".toByteArray(Charsets.UTF_8)
  }
}
