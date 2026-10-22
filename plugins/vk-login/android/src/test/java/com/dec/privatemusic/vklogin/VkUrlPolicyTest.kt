package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VkUrlPolicyTest {
    private fun ok(url: String) = assertTrue(url, VkUrlPolicy.isAllowedNavigation(url))
    private fun no(url: String) = assertFalse(url, VkUrlPolicy.isAllowedNavigation(url))

    @Test
    fun vkOverHttpsIsAllowed() {
        listOf(
            "https://vk.ru/login",
            "https://id.vk.com/auth",
            "https://login.vk.ru/?act=x",
            "https://VK.com/",
            "HTTPS://Vk.Ru/Login",
            "https://m.vk.ru",
            "https://vk.ru:443/login",
            "https://vk.ru:/login",
            "https://a.b.c.vk.com/x?y=1#z",
            "about:blank",
            "about:srcdoc",
            "ABOUT:BLANK",
            "blob:https://id.vk.ru/6b1f0f7e-0000-4000-8000-000000000000",
        ).forEach(::ok)
    }

    @Test
    fun lookalikeHostsAreRefused() {
        listOf(
            "https://vk.ru.evil.com/",
            "https://evilvk.ru/",
            "https://evil-vk.com/",
            "https://notvk.ru/",
            "https://vk.ru.com/",
            "https://example.com/?vk.ru",
            "https://example.com/vk.ru",
            "https://example.com#vk.ru",
            "https://vk.run/",
            "https://vk.ruu/",
            "https://xn--vk-6kc.ru/",
            "https://vk.ru./",
            "https://.vk.ru/",
            "https://vk..ru/",
            "https://vk%2eru/",
            "https://vk.ru%2eevil.com/",
        ).forEach(::no)
    }

    @Test
    fun idnLookalikesAreRefused() {
        // Cyrillic small ka (U+043A) for "k", Cyrillic "v", fullwidth letters, a dotless-i style host.
        listOf(
            "https://vк.ru/",
            "https://вk.ru/",
            "https://ｖｋ.ru/",
            "https://vk。ru/",
            "https://vk.ru․evil.com/",
            "https://vk.rú/",
        ).forEach(::no)
    }

    @Test
    fun userInfoTricksAreRefused() {
        listOf(
            "https://vk.ru@evil.com/",
            "https://vk.ru:pass@evil.com/",
            "https://evil.com@vk.ru/",
            "https://user@vk.ru/",
            "https://vk.ru%40evil.com/",
            "https://vk.ru\\@evil.com/",
            "https://vk.ru\\.evil.com/",
            "https://evil.com\\.vk.ru/",
        ).forEach(::no)
        // The host is vk.ru; "@evil.com/" is only a fragment.
        ok("https://vk.ru#@evil.com/")
        ok("https://vk.ru?u=@evil.com")
    }

    @Test
    fun portsOtherThan443AreRefused() {
        listOf(
            "https://vk.ru:8443/",
            "https://vk.ru:80/",
            "https://vk.ru:0443/",
            "https://vk.ru:443443/",
            "https://vk.ru:abc/",
            "https://vk.ru:443:443/",
        ).forEach(::no)
    }

    @Test
    fun otherSchemesAreRefused() {
        listOf(
            "http://vk.ru/login",
            "javascript:alert(1)",
            "JavaScript:alert(document.cookie)",
            "data:text/html,<script>1</script>",
            "file:///etc/passwd",
            "content://com.android.contacts/contacts",
            "intent://vk.ru/#Intent;scheme=https;package=com.evil;end",
            "intent:#Intent;action=android.intent.action.VIEW;end",
            "market://details?id=com.evil",
            "vkontakte://profile",
            "tel:+10000000",
            "sms:+10000000",
            "mailto:a@b.c",
            "ftp://vk.ru/",
            "ws://vk.ru/",
            "wss://vk.ru/",
            "about:config",
            "about:blank.evil",
            "about:",
            "blob:https://evil.example/123",
            "blob:http://vk.ru/123",
            "blob:blob:https://vk.ru/1",
            "blob:",
            "chrome://settings",
            "view-source:https://vk.ru/",
            "https:vk.ru",
            "https:/vk.ru/",
            "https:///vk.ru/",
            "//vk.ru/",
            "vk.ru",
            "https://",
            "",
            ":",
        ).forEach(::no)
        assertFalse(VkUrlPolicy.isAllowedNavigation(null))
    }

    @Test
    fun whitespaceAndControlCharactersAreRefused() {
        listOf(
            " https://vk.ru/",
            "https://vk.ru/ ",
            "https://vk.ru/\n",
            "https://vk.ru\t/",
            "https://vk.ru/\u0000",
            "https://vk.ru/\u007f",
            "https://vk.ru/é",
            "https://vk .ru/",
        ).forEach(::no)
        no("https://vk.ru/" + "a".repeat(9000))
    }

    @Test
    fun hostRules() {
        assertTrue(VkUrlPolicy.isVkHost("vk.ru"))
        assertTrue(VkUrlPolicy.isVkHost("LOGIN.VK.RU"))
        assertTrue(VkUrlPolicy.isVkHost("vk.com"))
        assertFalse(VkUrlPolicy.isVkHost(null))
        assertFalse(VkUrlPolicy.isVkHost(""))
        assertFalse(VkUrlPolicy.isVkHost("com"))
        assertFalse(VkUrlPolicy.isVkHost("ru"))
        assertFalse(VkUrlPolicy.isVkHost("vk.ru:443"))
        assertFalse(VkUrlPolicy.isVkHost("a_b.vk.ru"))
        assertFalse(VkUrlPolicy.isVkHost("x".repeat(260) + ".vk.ru"))
    }

    @Test
    fun hostOfAVkPage() {
        assertEquals("login.vk.ru", VkUrlPolicy.vkHostOf("https://Login.VK.ru:443/?act=web_token"))
        assertNull(VkUrlPolicy.vkHostOf("about:blank"))
        assertNull(VkUrlPolicy.vkHostOf("blob:https://id.vk.ru/1"))
        assertNull(VkUrlPolicy.vkHostOf("https://evil.com/"))
        assertNull(VkUrlPolicy.vkHostOf(null))
        assertTrue(VkUrlPolicy.isVkHttpsPage("https://vk.ru/"))
        assertFalse(VkUrlPolicy.isVkHttpsPage("about:blank"))
    }

    @Test
    fun logsNeverCarryPathQueryOrCredentials() {
        val secrets = listOf("hash=SECRET", "code=SECRET", "SECRET", "token", "pass")
        listOf(
            "https://vk.ru/login?hash=SECRET",
            "https://user:pass@id.vk.ru/path/SECRET?code=SECRET#token",
            "https://evil.com/SECRET?hash=SECRET",
            "http://evil.com:8080/SECRET",
            "intent://evil.com/SECRET#Intent;end",
            "javascript:SECRET",
            "data:text/html,SECRET",
            "blob:https://id.vk.ru/SECRET",
            "market://details?id=SECRET",
            "SECRET",
        ).forEach { url ->
            val line = VkUrlPolicy.describeForLog(url)
            secrets.forEach { assertFalse("$url -> $line", line.contains(it)) }
        }
        assertEquals("https page on id.vk.ru", VkUrlPolicy.describeForLog("https://user:pass@ID.vk.ru:443/a/b?c=d#e"))
        assertEquals("https page on evil.com", VkUrlPolicy.describeForLog("https://evil.com/SECRET"))
        assertEquals("intent page on evil.com", VkUrlPolicy.describeForLog("intent://evil.com/x"))
        assertEquals("javascript page", VkUrlPolicy.describeForLog("javascript:SECRET"))
        assertEquals("unknown page", VkUrlPolicy.describeForLog(null))
        assertEquals("unparsable page", VkUrlPolicy.describeForLog("no scheme here"))
    }

    @Test
    fun appStorageSurvivesTheWipe() {
        assertFalse(VkUrlPolicy.shouldWipeOrigin("http://tauri.localhost"))
        assertFalse(VkUrlPolicy.shouldWipeOrigin("https://tauri.localhost"))
        assertFalse(VkUrlPolicy.shouldWipeOrigin("http://localhost:1420"))
        assertFalse(VkUrlPolicy.shouldWipeOrigin("tauri://localhost"))
        assertFalse(VkUrlPolicy.shouldWipeOrigin("file://"))
        assertFalse(VkUrlPolicy.shouldWipeOrigin(null))
        assertTrue(VkUrlPolicy.shouldWipeOrigin("https://vk.ru"))
        assertTrue(VkUrlPolicy.shouldWipeOrigin("https://id.vk.com"))
        assertTrue(VkUrlPolicy.shouldWipeOrigin("https://mc.yandex.ru"))
        assertTrue(VkUrlPolicy.shouldWipeOrigin("https://localhost.evil.com"))
        assertTrue(VkUrlPolicy.shouldWipeOrigin("https://evil.com:8443"))
    }
}
