package com.dec.privatemusic.vklogin

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import androidx.exifinterface.media.ExifInterface
import java.io.ByteArrayOutputStream

internal object CoverImageDecoder {
    fun jpeg(bytes: ByteArray): ByteArray {
        require(bytes.size in 1..CoverImagePolicy.MAX_INPUT)
        val bounds=BitmapFactory.Options().apply {inJustDecodeBounds=true}
        BitmapFactory.decodeByteArray(bytes,0,bytes.size,bounds)
        val sample=CoverImagePolicy.sample(bounds.outWidth,bounds.outHeight)
        val options=BitmapFactory.Options().apply {inSampleSize=sample;inPreferredConfig=Bitmap.Config.ARGB_8888;inScaled=false}
        val bitmaps=mutableListOf<Bitmap>()
        try {
            var bitmap=BitmapFactory.decodeByteArray(bytes,0,bytes.size,options) ?: error("Unsupported image")
            bitmaps.add(bitmap)
            val exif=bytes.inputStream().use {ExifInterface(it).getAttributeInt(ExifInterface.TAG_ORIENTATION,ExifInterface.ORIENTATION_NORMAL)}
            val transform=CoverImagePolicy.orientation(exif)
            val matrix=Matrix().apply {
                postRotate(transform.rotation.toFloat())
                postScale(if(transform.flipX)-1f else 1f,if(transform.flipY)-1f else 1f)
            }
            if(!matrix.isIdentity) {
                bitmap=Bitmap.createBitmap(bitmap,0,0,bitmap.width,bitmap.height,matrix,true)
                bitmaps.add(bitmap)
            }
            val side=minOf(bitmap.width,bitmap.height)
            bitmap=Bitmap.createBitmap(bitmap,(bitmap.width-side)/2,(bitmap.height-side)/2,side,side)
            bitmaps.add(bitmap)
            if(side>1024) {bitmap=Bitmap.createScaledBitmap(bitmap,1024,1024,true);bitmaps.add(bitmap)}
            val output=ByteArrayOutputStream()
            check(bitmap.compress(Bitmap.CompressFormat.JPEG,88,output))
            return output.toByteArray().also {require(it.size in 5..CoverImagePolicy.MAX_JPEG)}
        } finally {bitmaps.distinct().forEach {if(!it.isRecycled)it.recycle()}}
    }
}
