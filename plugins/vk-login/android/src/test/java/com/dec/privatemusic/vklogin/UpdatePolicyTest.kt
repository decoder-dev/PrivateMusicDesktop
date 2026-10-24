package com.dec.privatemusic.vklogin
import org.junit.Assert.*
import org.junit.Test
class UpdatePolicyTest {
    @Test fun acceptsOnlySignedNewerVersionsOfThisPackage() {
        assertTrue(UpdatePolicy.accepts("music", "music", 605, 606, setOf("key"), setOf("key")))
        assertFalse(UpdatePolicy.accepts("music", "other", 605, 606, setOf("key"), setOf("key")))
        for (version in listOf(604L, 605L)) assertFalse(UpdatePolicy.accepts("music", "music", 605, version, setOf("key"), setOf("key")))
        assertFalse(UpdatePolicy.accepts("music", "music", 605, 606, setOf("key"), setOf("other")))
        assertFalse(UpdatePolicy.accepts("music", "music", 605, 606, emptySet(), emptySet()))
    }
    @Test fun rejectsUnsignedAndNonApkDownloads() {
        assertTrue(UpdatePolicy.validName("PrivateMusic_0.6.6_android-arm64.apk"))
        for (name in listOf("PrivateMusic_0.6.6_android-arm64-unsigned.apk", "../PrivateMusic_0.6.6_android-arm64.apk", "PrivateMusic_0.6.6_windows.exe", "random.apk")) assertFalse(UpdatePolicy.validName(name))
    }
}
