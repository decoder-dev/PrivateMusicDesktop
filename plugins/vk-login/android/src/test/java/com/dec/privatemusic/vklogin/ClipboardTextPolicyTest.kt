package com.dec.privatemusic.vklogin

import org.junit.Assert.*
import org.junit.Test

class ClipboardTextPolicyTest {
    @Test fun boundsUtf8RatherThanOnlyCharacters() {
        assertTrue(ClipboardTextPolicy.accepts("я".repeat(75000)))
        assertFalse(ClipboardTextPolicy.accepts("я".repeat(75001)))
        assertFalse(ClipboardTextPolicy.accepts("a".repeat(150001)))
    }
    @Test fun allowsPlainLyricsAndRejectsEmptyCopies() {
        assertTrue(ClipboardTextPolicy.accepts("Первая строка\nВторая <b>строка</b>"))
        assertFalse(ClipboardTextPolicy.accepts(""))
        assertFalse(ClipboardTextPolicy.accepts(" \n\t"))
    }
}
