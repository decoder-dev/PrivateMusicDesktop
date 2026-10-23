package com.dec.privatemusic.vklogin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class VkAppIntentPolicyTest {
    @Test
    fun acceptsOnlyVkAppIntents() {
        assertEquals("com.vkontakte.android",
            VkAppIntentPolicy.vkPackage("intent://qr.vk.ru/auth?x=1#Intent;scheme=https;package=com.vkontakte.android;end"))
        assertNull(VkAppIntentPolicy.vkPackage("intent://evil#Intent;scheme=https;package=com.evil.app;end"))
        assertNull(VkAppIntentPolicy.vkPackage("intent://qr.vk.ru/#Intent;scheme=https;package=com.vkontakte.android;component=com.vkontakte.android/.X;end"))
        assertNull(VkAppIntentPolicy.vkPackage("intent://qr.vk.ru/#Intent;scheme=https;SEL;package=com.vkontakte.android;end"))
        assertNull(VkAppIntentPolicy.vkPackage("intent://qr.vk.ru/#Intent;scheme=https;end"))
        assertNull(VkAppIntentPolicy.vkPackage("https://vk.ru/"))
        assertNull(VkAppIntentPolicy.vkPackage(null))
    }
}
