package com.dec.privatemusic.vklogin

import java.io.ByteArrayOutputStream
import java.io.InputStream

internal object CoverImagePolicy {
    const val MAX_INPUT = 10_000_000
    const val MAX_JPEG = 2_000_000
    fun read(input: InputStream): ByteArray {
        val output=ByteArrayOutputStream()
        val buffer=ByteArray(64*1024)
        var total=0
        while(true) {
            val count=input.read(buffer)
            if(count<0)break
            total+=count
            require(total<=MAX_INPUT) {"Image exceeds size limit"}
            output.write(buffer,0,count)
        }
        require(total>0)
        return output.toByteArray()
    }
    fun sample(width: Int,height: Int): Int {
        require(width in 1..32768 && height in 1..32768 && width.toLong()*height<=100_000_000L)
        var sample=1
        while(width/sample>1024 || height/sample>1024)sample*=2
        return sample
    }
    data class Transform(val rotation: Int=0,val flipX: Boolean=false,val flipY: Boolean=false)
    fun orientation(value: Int): Transform = when(value) {
        2->Transform(flipX=true)
        3->Transform(rotation=180)
        4->Transform(flipY=true)
        5->Transform(rotation=90,flipX=true)
        6->Transform(rotation=90)
        7->Transform(rotation=270,flipX=true)
        8->Transform(rotation=270)
        else->Transform()
    }
}
