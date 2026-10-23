package com.dec.privatemusic.vklogin

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import androidx.exifinterface.media.ExifInterface
import java.io.File
import kotlin.math.abs

/** Runs only in a debuggable app, against generated pixels and a disposable cache file. */
internal object CoverImageSelfTest {
    fun run(cache: File): String {
        val source=Bitmap.createBitmap(400,200,Bitmap.Config.ARGB_8888)
        val sourcePixels=IntArray(400*200) {index->
            val x=index%400;val y=index/400
            when {
                y<100 && x<200->Color.RED
                y<100->Color.GREEN
                x<200->Color.BLUE
                else->Color.YELLOW
            }
        }
        source.setPixels(sourcePixels,0,400,0,0,400,200)
        val cases=listOf(2 to listOf(Color.GREEN,Color.RED,Color.YELLOW,Color.BLUE),
            6 to listOf(Color.BLUE,Color.RED,Color.YELLOW,Color.GREEN),
            7 to listOf(Color.YELLOW,Color.GREEN,Color.BLUE,Color.RED))
        try {
            for((orientation,expected) in cases) {
                val file=File.createTempFile("pm-cover-selftest-",".jpg",cache)
                try {
                    file.outputStream().use {check(source.compress(Bitmap.CompressFormat.JPEG,95,it))}
                    ExifInterface(file).apply {
                        setAttribute(ExifInterface.TAG_ORIENTATION,orientation.toString())
                        setAttribute(ExifInterface.TAG_USER_COMMENT,"pm-generated-test")
                        saveAttributes()
                    }
                    check(ExifInterface(file).getAttribute(ExifInterface.TAG_USER_COMMENT)=="pm-generated-test") {"Fixture metadata was not written"}
                    val jpeg=CoverImageDecoder.jpeg(file.readBytes())
                    val bitmap=BitmapFactory.decodeByteArray(jpeg,0,jpeg.size) ?: error("Self-test JPEG could not decode")
                    try {
                        check(bitmap.width==200 && bitmap.height==200) {"Unexpected cover dimensions"}
                        val pixels=listOf(bitmap.getPixel(50,50),bitmap.getPixel(150,50),bitmap.getPixel(50,150),bitmap.getPixel(150,150))
                        for(index in pixels.indices) {
                            val actual=pixels[index];val target=expected[index]
                            check(abs(Color.red(actual)-Color.red(target))<60 && abs(Color.green(actual)-Color.green(target))<60 && abs(Color.blue(actual)-Color.blue(target))<60) {"Orientation $orientation, corner $index"}
                        }
                    } finally {bitmap.recycle()}
                    jpeg.inputStream().use {input->
                        val metadata=ExifInterface(input)
                        check(metadata.getAttribute(ExifInterface.TAG_USER_COMMENT)==null) {"Source metadata survived"}
                        check(metadata.getAttributeInt(ExifInterface.TAG_ORIENTATION,0) in 0..1) {"Orientation tag survived"}
                    }
                } finally {file.delete()}
            }
            return "ok"
        } finally {source.recycle()}
    }
}
