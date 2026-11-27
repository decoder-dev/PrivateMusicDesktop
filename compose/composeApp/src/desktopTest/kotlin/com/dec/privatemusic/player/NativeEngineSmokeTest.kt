package com.dec.privatemusic.player

import kotlin.test.Test
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking

/** Verifies that JNA can load the Rust resource without a developer JNA_LIBRARY_PATH. */
class NativeEngineSmokeTest {
    @Test
    fun packagedRustLibraryLoadsFromClasspath() = runBlocking {
        val previous = System.getProperty("pm.compose.demo")
        try {
            System.setProperty("pm.compose.demo", "0")
            val api = platformPlayerApi()
            assertTrue(api.state().status.isNotBlank())
            api.close()
        } finally {
            if (previous == null) System.clearProperty("pm.compose.demo")
            else System.setProperty("pm.compose.demo", previous)
        }
    }
}
