package com.dec.privatemusic.vklogin

import java.io.File

/** Only a prepared copy in our dedicated cache subtree can receive a read grant. */
internal object AudioSharePolicy {
    const val MAX_BYTES = 512L * 1024L * 1024L
    fun checked(cache: File, path: String): File {
        val input = File(path)
        val file = input.canonicalFile
        val root = File(cache, "audio-share").canonicalFile
        require(input.absoluteFile == file && file.parentFile?.parentFile == root)
        require(Regex("share-[a-f0-9]+-[a-f0-9]+").matches(file.parentFile!!.name))
        require(file.isFile && file.length() in 1..MAX_BYTES)
        require(file.extension in listOf("aac", "mp3"))
        return file
    }
}
