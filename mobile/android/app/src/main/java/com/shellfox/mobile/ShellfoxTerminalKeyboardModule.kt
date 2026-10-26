package com.shellfox.mobile

import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.UiThreadUtil

/** Shows the IME only after the local terminal page confirms its textarea has focus. */
class ShellfoxTerminalKeyboardModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context) {
  override fun getName() = "ShellfoxTerminalKeyboard"

  @ReactMethod
  fun show(promise: Promise) {
    UiThreadUtil.runOnUiThread {
      val focused = reactApplicationContext.currentActivity?.currentFocus
      if (focused !is WebView) {
        promise.reject("KEYBOARD_UNAVAILABLE", "Terminal is not focused.")
        return@runOnUiThread
      }
      val controller = ViewCompat.getWindowInsetsController(focused)
      if (controller == null) {
        promise.reject("KEYBOARD_UNAVAILABLE", "Terminal window is unavailable.")
        return@runOnUiThread
      }
      controller.show(WindowInsetsCompat.Type.ime())
      promise.resolve(null)
    }
  }
}
