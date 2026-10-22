package com.dec.privatemusic.vklogin

/** Twin of `load_verdict` in login.rs. */
enum class LoadVerdict { WAITING, FAILED }

object LoadWatch {
    /** How long VK's login page may take before the screen is called stuck. */
    const val LOAD_TIMEOUT_MS = 25_000L

    /**
     * A blank page can be the browser's own first one: only a non-VK page that is
     * still what we see after this long is taken for an error page.
     */
    const val ERROR_PAGE_GRACE_MS = 4_000L

    fun verdict(elapsedMs: Long, loaded: Boolean, errorPage: Boolean): LoadVerdict = when {
        loaded -> LoadVerdict.WAITING
        (errorPage && elapsedMs >= ERROR_PAGE_GRACE_MS) || elapsedMs >= LOAD_TIMEOUT_MS -> LoadVerdict.FAILED
        else -> LoadVerdict.WAITING
    }
}
