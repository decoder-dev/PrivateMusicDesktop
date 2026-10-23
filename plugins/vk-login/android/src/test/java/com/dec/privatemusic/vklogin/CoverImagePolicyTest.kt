package com.dec.privatemusic.vklogin

import org.junit.Assert.*
import org.junit.Test

class CoverImagePolicyTest {
    @Test fun image_reads_are_bounded_even_without_provider_metadata() {
        assertArrayEquals(byteArrayOf(1,2,3),CoverImagePolicy.read(byteArrayOf(1,2,3).inputStream()))
        assertTrue(runCatching {CoverImagePolicy.read(byteArrayOf().inputStream())}.isFailure)
        assertTrue(runCatching {CoverImagePolicy.read(ByteArray(CoverImagePolicy.MAX_INPUT+1).inputStream())}.isFailure)
    }
    @Test fun decoding_samples_large_images_before_allocating_pixels() {
        assertEquals(1,CoverImagePolicy.sample(600,900))
        assertEquals(4,CoverImagePolicy.sample(4032,3024))
        assertEquals(16,CoverImagePolicy.sample(10000,10000))
        for(size in listOf(0 to 1,1 to -1,50000 to 50000,32768 to 32768))
            assertTrue(runCatching {CoverImagePolicy.sample(size.first,size.second)}.isFailure)
    }
    @Test fun orientation_covers_rotations_and_mirrored_camera_images() {
        assertEquals(CoverImagePolicy.Transform(rotation=90),CoverImagePolicy.orientation(6))
        assertEquals(CoverImagePolicy.Transform(flipX=true),CoverImagePolicy.orientation(2))
        assertEquals(CoverImagePolicy.Transform(rotation=270,flipX=true),CoverImagePolicy.orientation(7))
        assertEquals(CoverImagePolicy.Transform(),CoverImagePolicy.orientation(0))
    }
}
