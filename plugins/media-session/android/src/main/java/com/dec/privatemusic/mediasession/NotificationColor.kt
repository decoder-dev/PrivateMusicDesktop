package com.dec.privatemusic.mediasession

/** Turns the artwork's average colour into a background that white text can sit on. */
object NotificationColor {
    private const val MAX_LUMINANCE = 0.30

    /** Opaque ARGB, darkened until its relative luminance is at most [MAX_LUMINANCE]. */
    fun forBackground(argb: Int): Int {
        var r = (argb shr 16) and 0xFF
        var g = (argb shr 8) and 0xFF
        var b = argb and 0xFF
        var guard = 0
        while (luminance(r, g, b) > MAX_LUMINANCE && guard++ < 32) {
            r = r * 9 / 10
            g = g * 9 / 10
            b = b * 9 / 10
        }
        return (0xFF shl 24) or (r shl 16) or (g shl 8) or b
    }

    fun luminance(r: Int, g: Int, b: Int): Double {
        fun channel(v: Int): Double {
            val s = v / 255.0
            return if (s <= 0.03928) s / 12.92 else Math.pow((s + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }
}
