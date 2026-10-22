package com.dec.privatemusic2.android

import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import kotlin.math.roundToInt

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var insetsScript: ScriptHandler? = null
  private var insetsJs: String = ""

  // The hardware Back button goes to the page first (see dispatchBack).
  override val handleBackNavigation: Boolean = false

  override fun onCreate(savedInstanceState: Bundle?) {
    // Page inspection from chrome://inspect only in debug builds.
    WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)

    val transparent = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT)
    enableEdgeToEdge(statusBarStyle = transparent, navigationBarStyle = transparent)
    super.onCreate(savedInstanceState)

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      window.attributes.layoutInDisplayCutoutMode =
        WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
    }

    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() = dispatchBack()
    })

    // Insets reach the page as CSS variables (--sat --sab --sal --sar, in CSS px;
    // --kb is the on-screen keyboard height). env(safe-area-inset-*) needs
    // viewport-fit=cover and a recent WebView, the variables always work.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      // Edge-to-edge windows are not resized for the keyboard by the system:
      // keep the page above it.
      view.setPadding(0, 0, 0, ime)
      val density = resources.displayMetrics.density
      fun css(px: Int) = ((px / density) * 10).roundToInt() / 10.0
      publishInsets(
        top = css(bars.top),
        bottom = css(if (ime > 0) 0 else bars.bottom),
        left = css(bars.left),
        right = css(bars.right),
        keyboard = css(ime),
      )
      insets
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    if (insetsJs.isNotEmpty()) installInsetsScript(webView)
    ViewCompat.requestApplyInsets(findViewById(android.R.id.content))
  }

  private fun publishInsets(top: Double, bottom: Double, left: Double, right: Double, keyboard: Double) {
    val set = "var s=document.documentElement.style;" +
      "s.setProperty('--sat','${top}px');s.setProperty('--sab','${bottom}px');" +
      "s.setProperty('--sal','${left}px');s.setProperty('--sar','${right}px');" +
      "s.setProperty('--kb','${keyboard}px');"
    // At document start the root element may not exist yet.
    insetsJs = "(function(){function set(){$set}" +
      "if(document.documentElement){set();}else{" +
      "new MutationObserver(function(_,o){if(document.documentElement){set();o.disconnect();}})" +
      ".observe(document,{childList:true});}})();"
    val view = webView ?: return
    installInsetsScript(view)
    view.evaluateJavascript(insetsJs, null)
  }

  // Applied again at the start of every page load, so a reload keeps the insets.
  private fun installInsetsScript(view: WebView) {
    if (!WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) return
    insetsScript?.remove()
    insetsScript = WebViewCompat.addDocumentStartJavaScript(
      view,
      insetsJs,
      setOf("http://tauri.localhost", "https://tauri.localhost"),
    )
  }

  /**
   * Back: the page gets a cancelable `pm-back` event on window. If it calls
   * preventDefault() it handled the press (closed a panel, went up a page);
   * otherwise the app goes to the background and the music keeps playing.
   */
  private fun dispatchBack() {
    val view = webView
    if (view == null) {
      moveTaskToBack(true)
      return
    }
    view.evaluateJavascript(
      "(function(){var e=new CustomEvent('pm-back',{cancelable:true});" +
        "window.dispatchEvent(e);return e.defaultPrevented;})()"
    ) { handled ->
      if (handled != "true") moveTaskToBack(true)
    }
  }
}
