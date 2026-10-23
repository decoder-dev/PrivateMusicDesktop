package com.dec.privatemusic.vklogin

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.net.http.SslError
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowManager
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.RenderProcessGoneDetail
import android.webkit.SslErrorHandler
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import java.io.ByteArrayInputStream
import java.io.File
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * The VK sign-in screen: a full-screen, isolated WebView on vk.ru's own login
 * page (so password, 2FA and captcha work as in a browser). Security and
 * behaviour mirror `src-tauri/src/login.rs`; see README.md for the full list.
 *
 * Everything runs on the main thread except the token request ([worker]).
 */
class LoginActivity : Activity() {
    companion object {
        const val EXTRA_MANUAL = "pm_manual_check"
        const val EXTRA_SMOKE_RESULT = "pm_smoke_result"

        /** One identity for the WebView and the token request (VK ties the token to it). */
        const val USER_AGENT =
            "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36"

        /**
         * Third-party cookies are ON. VK's sign-in hands the session between its own domains (vk.com, id.vk.com,
         * login.vk.com, vk.ru) inside frames; without them the page says "signed in" while the cookie that the
         * app needs (remixsid on vk.ru) is never set, and sign-in hangs. Desktop browsers allow them by default;
         * Android's WebView does not. Only VK hosts can be navigated to, and the browser is wiped afterwards.
         */
        const val ALLOW_THIRD_PARTY_COOKIES = true

        private const val TAG = "VkLogin"
        private const val POLL_MS = 1500L
        private const val UNCONFIRMED_BACKOFF_MS = 5000L
        private const val SMOKE_TIMEOUT_MS = 60_000L
    }

    private val main = Handler(Looper.getMainLooper())
    private val worker: ExecutorService = Executors.newSingleThreadExecutor()
    private var webView: WebView? = null

    private lateinit var root: LinearLayout
    private lateinit var bar: LinearLayout
    private lateinit var titleView: TextView
    private lateinit var closeButton: Button
    private lateinit var doneButton: Button
    private lateinit var noticePanel: LinearLayout
    private lateinit var noticeText: TextView
    private lateinit var retryButton: Button

    private var smoke = false
    private var finished = false

    // Did VK's own page arrive? (login.rs: PAGE_LOADED / PAGE_ERROR / LOAD_FAILED)
    private var pageLoaded = false
    private var pageError = false
    private var loadFailed = false
    private var mainHttpError = false
    private var loadStartedAt = 0L

    // The session check (login.rs: poll)
    private var manual = false
    private var exchanging = false
    private var failures = 0
    private var lastCookieCount = -1
    private var lastHeader: String? = null
    private var notBefore = 0L

    private var backCallback: OnBackInvokedCallback? = null

    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        // Process death: Android re-creates this screen in a fresh process where
        // nobody awaits its result any more (README, "Process death"). Close it.
        if (savedInstanceState != null && !LoginBridge.live) {
            setResult(RESULT_CANCELED)
            finish()
            return
        }
        // No screenshots, no recents thumbnail of a password field.
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        smoke = SmokeMode.requested(
            intent.getBooleanExtra(SmokeMode.EXTRA, false),
            intent.getStringExtra(SmokeMode.EXTRA),
            isDebuggable(),
        )
        LoginBridge.live = true
        LoginBridge.cancelRequest = { main.post { closeRequested() } }
        LoginBridge.recheck = {
            main.post {
                if (!finished) {
                    if (loadFailed) retryLoad()
                    checkNow()
                }
            }
        }
        buildUi()
        registerBack()
        if (smoke) main.postDelayed({ finishSmoke("error timeout: no VK https page finished loading", null) }, SMOKE_TIMEOUT_MS)
        manual = intent.getBooleanExtra(EXTRA_MANUAL, false)
        // Every sign-in starts from a clean browser; a re-created screen (the
        // system destroyed it, the process lived on) keeps what it had.
        if (savedInstanceState != null) startLoading()
        else BrowsingData.wipe(webView) { if (!finished) startLoading() }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (finished || !intent.getBooleanExtra(EXTRA_MANUAL, false)) return
        // The app asked again ("return to sign-in"): retry a page that never came,
        // and check the session now.
        if (loadFailed) retryLoad()
        checkNow()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        // Declared in the manifest: rotation and theme changes keep the page.
        super.onConfigurationChanged(newConfig)
        applyTheme()
    }

    @Suppress("OVERRIDE_DEPRECATION")
    override fun onBackPressed() {
        handleBack()
    }

    override fun onDestroy() {
        super.onDestroy()
        main.removeCallbacksAndMessages(null)
        if (isFinishing) {
            if (!finished) {
                // Swiped away or closed by the system: report it, then clean up.
                finished = true
                LoginBridge.emit(if (loadFailed) LoginEvent.LoadFailed else LoginEvent.Cancelled)
            }
            BrowsingData.wipe(webView) {}
            LoginBridge.live = false
            LoginBridge.cancelRequest = null
            LoginBridge.recheck = null
            LoginBridge.sink = null
        }
        backCallback?.let { if (Build.VERSION.SDK_INT >= 33) onBackInvokedDispatcher.unregisterOnBackInvokedCallback(it) }
        worker.shutdownNow()
        webView?.let {
            (it.parent as? ViewGroup)?.removeView(it)
            it.stopLoading()
            it.webViewClient = WebViewClient()
            it.destroy()
        }
        webView = null
    }

    // ---- loading and the load watchdog --------------------------------------------------

    private fun startLoading() {
        val wv = webView ?: return
        pageLoaded = false
        pageError = false
        loadFailed = false
        mainHttpError = false
        loadStartedAt = SystemClock.elapsedRealtime()
        wv.loadUrl(VkUrlPolicy.LOGIN_URL)
        main.removeCallbacks(tick)
        main.postDelayed(tick, POLL_MS)
    }

    private fun retryLoad() {
        showNotice(false)
        webView?.stopLoading()
        startLoading()
    }

    private val tick = object : Runnable {
        override fun run() {
            if (finished) return
            try {
                onTick()
            } catch (e: Exception) {
                Log.w(TAG, "check failed: ${e.javaClass.simpleName}")
            }
            if (!finished) main.postDelayed(this, POLL_MS)
        }
    }

    private fun checkNow() {
        manual = true
        main.removeCallbacks(tick)
        main.post(tick)
    }

    private fun onTick() {
        val now = SystemClock.elapsedRealtime()
        if (!loadFailed && !smoke &&
            LoadWatch.verdict(now - loadStartedAt, pageLoaded, pageError) == LoadVerdict.FAILED
        ) {
            // VK's page never came (a VPN that is on, or should be off): tell the app,
            // and show the same hint here, with a Retry.
            Log.w(TAG, "the VK login page did not load")
            loadFailed = true
            showNotice(true)
            LoginBridge.emit(LoginEvent.LoadFailed)
        }
        if (smoke || exchanging || now < notBefore) return

        val manager = CookieManager.getInstance()
        val loginCookies = manager.getCookie(VkUrlPolicy.TOKEN_COOKIE_URL)
        val pageCookies = manager.getCookie(VkUrlPolicy.PAGE_COOKIE_URL)
        val pairs = SessionCookies.merge(loginCookies, pageCookies)
        if (pairs.size != lastCookieCount) {
            lastCookieCount = pairs.size
            LoginTrace.note("cookie names: login.vk.ru=[" + SessionCookies.parse(loginCookies).joinToString(",") { it.first } +
                "] vk.ru=[" + SessionCookies.parse(pageCookies).joinToString(",") { it.first } + "]")
        }
        val wasManual = manual
        manual = false
        val header = SessionCookies.sessionHeader(pairs, wasManual)
        if (header == null) {
            if (wasManual) LoginBridge.emit(LoginEvent.Pending())
            return
        }
        if (header != lastHeader) {
            failures = 0
            lastHeader = header
            Log.i(TAG, "session detected, exchanging token")
            LoginTrace.note("session cookies seen: " + pairs.joinToString(",") { it.first } + " (third-party cookies " + (if (ALLOW_THIRD_PARTY_COOKIES) "on" else "off") + ")")
        }
        exchanging = true
        worker.execute {
            val outcome = try {
                WebTokenExchange.exchange(header, USER_AGENT)
            } catch (e: Exception) {
                ExchangeOutcome.Failed("network")
            }
            main.post { onExchange(outcome, header, wasManual) }
        }
    }

    private fun onExchange(outcome: ExchangeOutcome, header: String, wasManual: Boolean) {
        exchanging = false
        LoginTrace.note("token exchange: " + when (outcome) { is ExchangeOutcome.Okay -> "okay"; ExchangeOutcome.Unauthorized -> "not signed in yet"; is ExchangeOutcome.Failed -> "failed (${outcome.kind})" })
        // Closing the screen during the request cancels adoption.
        if (finished) return
        when (outcome) {
            is ExchangeOutcome.Okay -> {
                Log.i(TAG, "token exchange succeeded")
                finishWith(LoginEvent.Success(header, outcome.json, outcome.setCookies, USER_AGENT))
            }
            // A cookie exists but VK does not call it signed in yet (a 2FA or captcha step): keep waiting.
            ExchangeOutcome.Unauthorized -> {
                failures++
                if (failures == 3 || wasManual) {
                    Log.w(TAG, "VK has not confirmed the web session")
                    LoginBridge.emit(LoginEvent.Pending())
                }
                notBefore = SystemClock.elapsedRealtime() + UNCONFIRMED_BACKOFF_MS
            }
            is ExchangeOutcome.Failed -> {
                failures++
                if (failures >= 3) finishWith(LoginEvent.Error(outcome.kind))
            }
        }
    }

    // ---- finishing ------------------------------------------------------------------

    /** The one way out with a result. The session (if any) goes to Rust, then the browser is wiped. */
    private fun finishWith(event: LoginEvent) {
        if (finished) return
        finished = true
        main.removeCallbacksAndMessages(null)
        LoginBridge.emit(event)
        BrowsingData.wipe(webView) {}
        setResult(RESULT_OK)
        finish()
    }

    private fun closeRequested() {
        // After a page that never loaded, closing leaves the app on its VPN hint
        // (loadfailed), like the desktop window that stays hidden in that state.
        finishWith(if (loadFailed) LoginEvent.LoadFailed else LoginEvent.Cancelled)
    }

    private fun handleBack() {
        val wv = webView
        if (!finished && wv != null && wv.canGoBack()) wv.goBack() else closeRequested()
    }

    private fun registerBack() {
        if (Build.VERSION.SDK_INT >= 33) {
            val callback = OnBackInvokedCallback { handleBack() }
            backCallback = callback
            onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback)
        }
    }

    // ---- smoke mode (CI) ------------------------------------------------------------

    private fun smokePageLoaded(host: String) {
        if (finished) return
        // As on the desktop: prove the private cookie store works, not only the page.
        val cookies = CookieManager.getInstance()
        val url = VkUrlPolicy.TOKEN_COOKIE_URL
        cookies.setCookie(url, "pm_probe=ok; Path=/; Secure; HttpOnly") { _ ->
            val seen = SessionCookies.parse(cookies.getCookie(url)).any { it.first == "pm_probe" && it.second == "ok" }
            cookies.setCookie(url, "pm_probe=; Path=/; Max-Age=0") { _ ->
                if (seen) finishSmoke(SmokeMode.loadedLine(host), host)
                else finishSmoke("error private cookie store probe failed", null)
            }
        }
    }

    private fun finishSmoke(line: String, host: String?) {
        if (finished) return
        finished = true
        main.removeCallbacksAndMessages(null)
        Log.i("PmSmokeLogin", line)
        try {
            File(filesDir, SmokeMode.RESULT_FILE).writeText(line + "\n")
        } catch (e: Exception) {
            Log.w(TAG, "could not write the smoke result")
        }
        if (host != null) LoginBridge.emit(LoginEvent.SmokeLoaded(host))
        BrowsingData.wipe(webView) {}
        setResult(RESULT_OK, Intent().putExtra(EXTRA_SMOKE_RESULT, line))
        finish()
    }

    // ---- the WebView ----------------------------------------------------------------

    private fun createWebView(): WebView {
        val wv = WebView(this)
        wv.settings.apply {
            @SuppressLint("SetJavaScriptEnabled") javaScriptEnabled = true // VK's login page is a script app
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            @Suppress("DEPRECATION") allowFileAccessFromFileURLs = false
            @Suppress("DEPRECATION") allowUniversalAccessFromFileURLs = false
            @Suppress("DEPRECATION") savePassword = false
            @Suppress("DEPRECATION") saveFormData = false
            setGeolocationEnabled(false)
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            safeBrowsingEnabled = true
            cacheMode = WebSettings.LOAD_DEFAULT
            // window.open() reaches onCreateWindow, which refuses; nothing opens by itself.
            setSupportMultipleWindows(true)
            javaScriptCanOpenWindowsAutomatically = false
            mediaPlaybackRequiresUserGesture = true
            setSupportZoom(false)
            userAgentString = USER_AGENT
        }
        val cookies = CookieManager.getInstance()
        cookies.setAcceptCookie(true)
        cookies.setAcceptThirdPartyCookies(wv, ALLOW_THIRD_PARTY_COOKIES)
        // No downloads, no JS bridge object (nothing is read through one).
        wv.setDownloadListener { url, _, _, _, _ ->
            Log.w(TAG, "refused a download from ${VkUrlPolicy.describeForLog(url)}")
        }
        wv.webViewClient = LoginClient()
        wv.webChromeClient = LoginChromeClient()
        return wv
    }

    private inner class LoginClient : WebViewClient() {
        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            val url = request.url.toString()
            if (VkUrlPolicy.isAllowedNavigation(url)) return false
            // Only the host: sign-in addresses can carry one-time codes. This also
            // stops intent:, market: and every custom scheme.
            Log.w(TAG, "blocked a navigation: ${VkUrlPolicy.describeForLog(url)}")
            LoginTrace.note("BLOCKED navigation: ${VkUrlPolicy.describeForLog(url)}")
            return true
        }

        override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
            val url = request.url.toString()
            val scheme = request.url.scheme?.lowercase()
            // The page's own images, scripts and CDN calls may come from other https hosts (the
            // desktop restricts navigations only); but never in clear, and never a top-level
            // document (redirect hops included) outside VK.
            val clear = scheme == "http" || scheme == "ws"
            val strayDocument = request.isForMainFrame && !VkUrlPolicy.isAllowedNavigation(url)
            if (clear || strayDocument) {
                Log.w(TAG, "blocked a request: ${VkUrlPolicy.describeForLog(url)}")
                return WebResourceResponse("text/plain", "utf-8", 403, "Blocked", emptyMap(), ByteArrayInputStream(ByteArray(0)))
            }
            return null
        }

        override fun onPageStarted(view: WebView, url: String?, favicon: android.graphics.Bitmap?) {
            mainHttpError = false
        }

        override fun onPageFinished(view: WebView, url: String?) {
            if (finished) return
            Log.i(TAG, "page loaded: ${VkUrlPolicy.describeForLog(url)}")
            LoginTrace.note("page loaded: ${VkUrlPolicy.describeForLog(url)}")
            val host = VkUrlPolicy.vkHostOf(url)
            if (host != null && !(mainHttpError && !pageLoaded)) {
                pageLoaded = true
                pageError = false
                if (loadFailed) {
                    // It got through after all (the VPN was switched meanwhile).
                    loadFailed = false
                    showNotice(false)
                    LoginBridge.emit(LoginEvent.LoadOk)
                }
                if (smoke) smokePageLoaded(host)
            } else if (!pageLoaded && url?.startsWith("about:") != true) {
                // Not VK's page: the browser's own error page, or a refused one.
                pageError = true
            }
        }

        override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
            if (request.isForMainFrame && !pageLoaded) {
                Log.w(TAG, "main frame error ${error.errorCode} on ${VkUrlPolicy.describeForLog(request.url.toString())}")
                pageError = true
            }
        }

        override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, errorResponse: WebResourceResponse) {
            if (request.isForMainFrame && !pageLoaded) {
                Log.w(TAG, "main frame http ${errorResponse.statusCode} on ${VkUrlPolicy.describeForLog(request.url.toString())}")
                mainHttpError = true
            }
        }

        override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
            // Never proceed, whatever the certificate problem.
            handler.cancel()
            Log.w(TAG, "refused a bad certificate (${error.primaryError}) on ${VkUrlPolicy.describeForLog(error.url)}")
            if (!pageLoaded) pageError = true
        }

        override fun onReceivedClientCertRequest(view: WebView, request: android.webkit.ClientCertRequest) {
            request.cancel()
        }

        override fun onReceivedHttpAuthRequest(view: WebView, handler: android.webkit.HttpAuthHandler, host: String?, realm: String?) {
            handler.cancel()
        }

        override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
            Log.w(TAG, "the web view's renderer died")
            main.post { finishWith(LoginEvent.Error("renderer")) }
            return true
        }
    }

    private inner class LoginChromeClient : WebChromeClient() {
        override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, resultMsg: android.os.Message?): Boolean {
            Log.w(TAG, "refused a pop-up window")
            return false
        }

        override fun onPermissionRequest(request: PermissionRequest) {
            request.deny()
        }

        override fun onGeolocationPermissionsShowPrompt(origin: String?, callback: GeolocationPermissions.Callback) {
            callback.invoke(origin, false, false)
        }

        override fun onShowFileChooser(
            webView: WebView,
            filePathCallback: ValueCallback<Array<android.net.Uri>>,
            fileChooserParams: FileChooserParams,
        ): Boolean {
            filePathCallback.onReceiveValue(null)
            return true
        }

        override fun onConsoleMessage(consoleMessage: android.webkit.ConsoleMessage?): Boolean = true
    }

    // ---- the screen -----------------------------------------------------------------

    private fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    private fun buildUi() {
        closeButton = Button(this).apply {
            text = "✕"
            textSize = 20f
            contentDescription = getString(R.string.pm_vk_login_close)
            minWidth = dp(48); minimumWidth = dp(48); minHeight = dp(48); minimumHeight = dp(48)
            background = null
            setOnClickListener { closeRequested() }
        }
        titleView = TextView(this).apply {
            text = getString(R.string.pm_vk_login_title)
            textSize = 18f
            typeface = Typeface.DEFAULT_BOLD
            gravity = Gravity.CENTER
        }
        androidx.core.view.ViewCompat.setAccessibilityHeading(titleView, true)
        doneButton = Button(this).apply {
            text = getString(R.string.pm_vk_login_done)
            isAllCaps = false
            minHeight = dp(48); minimumHeight = dp(48)
            background = null
            setOnClickListener { checkNow() }
        }
        bar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(closeButton, LinearLayout.LayoutParams(dp(56), dp(56)))
            addView(titleView, LinearLayout.LayoutParams(0, dp(56), 1f))
            addView(doneButton, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(56)))
        }
        val wv = createWebView()
        webView = wv
        noticeText = TextView(this).apply {
            text = getString(R.string.pm_vk_login_load_failed)
            textSize = 16f
            gravity = Gravity.CENTER
            setPadding(dp(24), dp(24), dp(24), dp(16))
        }
        retryButton = Button(this).apply {
            text = getString(R.string.pm_vk_login_retry)
            isAllCaps = false
            setOnClickListener { retryLoad() }
        }
        noticePanel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            visibility = View.GONE
            isClickable = true
            addView(noticeText)
            addView(retryButton)
        }
        val content = FrameLayout(this).apply {
            addView(wv, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            addView(noticePanel, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(bar, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(56)))
            addView(content, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            setOnApplyWindowInsetsListener { v, insets -> applyInsets(v, insets) }
        }
        setContentView(root)
        applyTheme()
    }

    private fun applyInsets(v: View, insets: WindowInsets): WindowInsets {
        if (Build.VERSION.SDK_INT >= 30) {
            val i = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout() or WindowInsets.Type.ime())
            v.setPadding(i.left, i.top, i.right, i.bottom)
        } else {
            @Suppress("DEPRECATION")
            v.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
        }
        return insets
    }

    private fun applyTheme() {
        val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val background = if (night) Color.parseColor("#121212") else Color.WHITE
        val barColor = if (night) Color.parseColor("#1E1E1E") else Color.parseColor("#F2F2F2")
        val text = if (night) Color.parseColor("#EEEEEE") else Color.parseColor("#1B1B1B")
        root.setBackgroundColor(background)
        bar.setBackgroundColor(barColor)
        noticePanel.setBackgroundColor(background)
        webView?.setBackgroundColor(background)
        listOf(titleView, noticeText).forEach { it.setTextColor(text) }
        listOf(closeButton, doneButton, retryButton).forEach {
            it.setTextColor(text)
            it.backgroundTintList = ColorStateList.valueOf(barColor)
        }
        retryButton.setBackgroundColor(barColor)
    }

    private fun showNotice(show: Boolean) {
        noticePanel.visibility = if (show) View.VISIBLE else View.GONE
    }

    private fun isDebuggable() = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
}
