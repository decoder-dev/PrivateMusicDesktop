package com.dec.privatemusic.vklogin

/**
 * CI smoke mode (the Android twin of the desktop `PM_SMOKE_LOGIN`): start the
 * host app's launcher activity with `--ez pm_smoke_login true` (or
 * `--es pm_smoke_login 1`). Honoured in debuggable builds only.
 */
object SmokeMode {
    const val EXTRA = "pm_smoke_login"
    const val RESULT_FILE = "pm_smoke_login.txt"

    fun requested(boolExtra: Boolean, stringExtra: String?, debuggable: Boolean): Boolean =
        debuggable && (boolExtra || stringExtra == "1" || stringExtra.equals("true", ignoreCase = true))

    fun loadedLine(host: String) = "loaded $host"
}
