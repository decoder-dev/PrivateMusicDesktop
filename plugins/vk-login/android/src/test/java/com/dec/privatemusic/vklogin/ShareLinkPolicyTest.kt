package com.dec.privatemusic.vklogin

import org.junit.Assert.*
import org.junit.Test

class ShareLinkPolicyTest {
    @Test fun acceptsOnlyVkResourceLinks() {
        for (url in listOf("https://vk.com/audio-7_4", "https://vk.com/music/album/7_2?access_key=abc", "https://vk.com/music/playlist/7_3")) {
            assertTrue(url, ShareLinkPolicy.accepts(url))
        }
        for (url in listOf("https://evil.invalid/audio7_2", "https://vk.com/audio7_2?access_token=secret", "https://u@vk.com/audio7_2", "https://vk.com/audio0_1", "https://vk.com/audio7_0", "https://vk.com/music/album/7_2?access_key=abc&cookie=secret", "https://vk.com/audio7_2#fragment")) {
            assertFalse(url, ShareLinkPolicy.accepts(url))
        }
    }
}
