package com.dec.privatemusic.vklogin

import java.util.Locale

/**
 * "Sign in with the VK app": VK's page asks to open `intent://qr.vk.ru/...#Intent;...;package=com.vkontakte.android;end`.
 * Only an intent: link whose target package is VK's own app is let through, and only to that app (the caller strips
 * component and selector and adds the BROWSABLE category). Everything else stays blocked. Pure logic, JVM-tested.
 */
object VkAppIntentPolicy {
    val VK_PACKAGES = setOf("com.vkontakte.android", "com.vk.im", "com.vk.vkvideo")

    /** The package named by an `intent:` link, or null when the link is not an acceptable VK app intent. */
    fun vkPackage(url: String?): String? {
        if (url == null || url.length > 4096 || !url.startsWith("intent:", ignoreCase = true)) return null
        val fragment = url.substringAfter("#Intent;", "")
        if (fragment.isEmpty() || !fragment.endsWith("end")) return null
        val fields = fragment.removeSuffix("end").split(';').filter { it.isNotEmpty() }
        // A component or a selector would target a specific class: never accepted.
        if (fields.any { it.startsWith("component=") || it.startsWith("SEL") }) return null
        val pkg = fields.firstOrNull { it.startsWith("package=") }?.substringAfter('=')?.lowercase(Locale.ROOT) ?: return null
        return pkg.takeIf { it in VK_PACKAGES }
    }
}
