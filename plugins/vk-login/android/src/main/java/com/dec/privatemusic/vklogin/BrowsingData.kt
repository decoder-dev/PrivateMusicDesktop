package com.dec.privatemusic.vklogin

import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView

/**
 * Wipes what a sign-in leaves in the WebView profile. Android has one cookie
 * jar and one web-storage store per app, shared with the host's own WebView, so:
 * cookies and the HTTP cache go entirely (the Tauri UI uses neither), while
 * web storage is deleted per origin and skips the app's own localhost origins
 * (see [VkUrlPolicy.shouldWipeOrigin]) so UI settings in localStorage survive.
 */
object BrowsingData {
    /** Calls [done] on the calling (main) thread once storage and cookies are gone. */
    fun wipe(webView: WebView?, done: () -> Unit) {
        var called = false
        val finish = {
            if (!called) {
                called = true
                done()
            }
        }
        try {
            webView?.apply {
                clearCache(true)
                clearHistory()
                @Suppress("DEPRECATION") clearFormData()
            }
        } catch (_: Exception) {
            // Best effort: storage and cookies below are the part that matters.
        }
        val removeCookies = {
            try {
                val cookies = CookieManager.getInstance()
                cookies.removeAllCookies {
                    try { cookies.flush() } catch (_: Exception) {}
                    finish()
                }
            } catch (_: Exception) {
                finish()
            }
        }
        try {
            wipeWebStorage(removeCookies)
        } catch (_: Exception) {
            removeCookies()
        }
    }

    @Suppress("DEPRECATION")
    private fun wipeWebStorage(then: () -> Unit) {
        val storage = WebStorage.getInstance()
        storage.getOrigins { origins ->
            try {
                origins?.keys?.filterIsInstance<String>()?.filter { VkUrlPolicy.shouldWipeOrigin(it) }?.forEach { storage.deleteOrigin(it) }
            } catch (_: Exception) {
            }
            then()
        }
    }
}
