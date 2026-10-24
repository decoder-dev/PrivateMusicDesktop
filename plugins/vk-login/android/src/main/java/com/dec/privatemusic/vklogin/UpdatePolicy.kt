package com.dec.privatemusic.vklogin

internal object UpdatePolicy {
    fun validName(name: String): Boolean = Regex("PrivateMusic_[0-9]+\\.[0-9]+\\.[0-9]+_android-arm64\\.apk").matches(name)
    fun accepts(expectedPackage: String, candidatePackage: String?, current: Long, candidate: Long, installedSigners: Set<String>, candidateSigners: Set<String>): Boolean =
        candidatePackage == expectedPackage && candidate > current && installedSigners.isNotEmpty() && installedSigners == candidateSigners
}
